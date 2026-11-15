//! NVIDIA GPU use per app and device totals, through NVML loaded at runtime.
//!
//! A suspended dGPU is never touched: its runtime PM status is read from sysfs first, and NVML is
//! loaded and initialised only when the GPU is already awake. NVML is shut down after every read,
//! because an open NVML session keeps the GPU from suspending again.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_int, c_uint, c_ulonglong, c_void, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use libloading::Library;

#[derive(Debug, Clone, PartialEq)]
pub enum Nvidia {
    /// Runtime-suspended; NVML was not loaded so it stays asleep.
    Asleep,
    Awake(Gpu),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub vram_used_bytes: u64,
    pub vram_total_bytes: u64,
    /// Device busy %, None where the driver doesn't report it.
    pub utilization_percent: Option<u32>,
    pub power_milliwatts: Option<u32>,
    /// Keyed by cgroup path relative to the cgroup root, like `cpu_io`.
    pub apps: BTreeMap<PathBuf, AppUsage>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AppUsage {
    pub vram_bytes: u64,
    /// Summed over the group's processes, each averaged over the sampling window.
    pub sm_percent: f64,
}

/// What one process uses, as NVML reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ProcUsage {
    vram_bytes: u64,
    sm_percent: f64,
}

/// The NVIDIA GPU's state, or None when there is no NVIDIA GPU or no usable driver.
/// SM % is averaged over the last `window` (pass the refresh interval).
pub fn read(window: Duration) -> Option<Nvidia> {
    let (bus_id, asleep) = find_gpu(Path::new("/sys/bus/pci/devices"))?;
    if asleep {
        return Some(Nvidia::Asleep);
    }
    // The GPU can suspend between the status check and NVML init; then init wakes it once.
    // That window is a few milliseconds per refresh and can't be closed from user space.
    let (gpu, procs) = Nvml::open()?.read(&bus_id, window)?;
    Some(Nvidia::Awake(Gpu {
        apps: per_app(Path::new("/proc"), &procs),
        ..gpu
    }))
}

/// PCI address of the first NVIDIA display controller under `pci_root`, and whether it is
/// runtime-suspended. Assumes one NVIDIA GPU, the laptop dGPU case this app targets.
fn find_gpu(pci_root: &Path) -> Option<(String, bool)> {
    let mut devices: Vec<_> = fs::read_dir(pci_root).ok()?.flatten().collect();
    devices.sort_by_key(|e| e.file_name());
    devices.into_iter().find_map(|entry| {
        let dir = entry.path();
        let read = |f: &str| fs::read_to_string(dir.join(f)).unwrap_or_default();
        let display = read("class").trim().starts_with("0x03");
        if read("vendor").trim() != "0x10de" || !display {
            return None;
        }
        let status = read("power/runtime_status");
        let asleep = matches!(status.trim(), "suspended" | "suspending");
        Some((entry.file_name().to_string_lossy().into_owned(), asleep))
    })
}

/// Sums the processes' use per cgroup. Processes that exited since NVML listed them are skipped.
fn per_app(proc_root: &Path, procs: &BTreeMap<u32, ProcUsage>) -> BTreeMap<PathBuf, AppUsage> {
    let mut apps = BTreeMap::<PathBuf, AppUsage>::new();
    for (pid, usage) in procs {
        let Some(group) = fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup"))
            .ok()
            .and_then(|s| parse_cgroup(&s))
        else {
            continue;
        };
        let app = apps.entry(group).or_default();
        app.vram_bytes += usage.vram_bytes;
        app.sm_percent += usage.sm_percent;
    }
    apps
}

/// The cgroup v2 path from `/proc/<pid>/cgroup` (`0::/user.slice/x.scope`), relative to the root.
fn parse_cgroup(content: &str) -> Option<PathBuf> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("0::/"))
        .map(PathBuf::from)
}

// NVML ABI, from nvml.h.
type Device = *mut c_void;
type Ret = c_int;
const SUCCESS: Ret = 0;
const INSUFFICIENT_SIZE: Ret = 7;
/// usedGpuMemory when the driver can't tell (e.g. without permission).
const VALUE_NOT_AVAILABLE: c_ulonglong = c_ulonglong::MAX;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessInfo {
    pid: c_uint,
    used_gpu_memory: c_ulonglong,
    gpu_instance_id: c_uint,
    compute_instance_id: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UtilizationSample {
    pid: c_uint,
    time_stamp: c_ulonglong,
    sm_util: c_uint,
    mem_util: c_uint,
    enc_util: c_uint,
    dec_util: c_uint,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: c_ulonglong,
    free: c_ulonglong,
    used: c_ulonglong,
}

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

type ProcessList = unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Ret;

/// An initialised NVML session; shut down on drop.
struct Nvml(&'static Library);

impl Nvml {
    fn open() -> Option<Self> {
        // Loaded once and kept: the library alone holds no device open, only `nvmlInit` does.
        static LIB: OnceLock<Option<Library>> = OnceLock::new();
        let lib = LIB
            .get_or_init(|| unsafe { Library::new("libnvidia-ml.so.1") }.ok())
            .as_ref()?;
        let nvml = Nvml(lib);
        let init: unsafe extern "C" fn() -> Ret = nvml.sym(b"nvmlInit_v2\0")?;
        (unsafe { init() } == SUCCESS).then_some(nvml)
    }

    fn sym<T: Copy>(&self, name: &[u8]) -> Option<T> {
        unsafe { self.0.get::<T>(name) }.ok().map(|s| *s)
    }

    fn read(&self, bus_id: &str, window: Duration) -> Option<(Gpu, BTreeMap<u32, ProcUsage>)> {
        unsafe {
            let by_bus: unsafe extern "C" fn(*const c_char, *mut Device) -> Ret =
                self.sym(b"nvmlDeviceGetHandleByPciBusId_v2\0")?;
            let bus_id = CString::new(bus_id).ok()?;
            let mut dev: Device = std::ptr::null_mut();
            if by_bus(bus_id.as_ptr(), &mut dev) != SUCCESS {
                return None;
            }

            let memory_info: unsafe extern "C" fn(Device, *mut Memory) -> Ret =
                self.sym(b"nvmlDeviceGetMemoryInfo\0")?;
            let mut mem = Memory::default();
            if memory_info(dev, &mut mem) != SUCCESS {
                return None;
            }
            let rates: unsafe extern "C" fn(Device, *mut Utilization) -> Ret =
                self.sym(b"nvmlDeviceGetUtilizationRates\0")?;
            let mut util = Utilization::default();
            let utilization_percent = (rates(dev, &mut util) == SUCCESS).then_some(util.gpu);
            let power: unsafe extern "C" fn(Device, *mut c_uint) -> Ret =
                self.sym(b"nvmlDeviceGetPowerUsage\0")?;
            let mut mw = 0;
            let power_milliwatts = (power(dev, &mut mw) == SUCCESS).then_some(mw);

            let mut procs = BTreeMap::<u32, ProcUsage>::new();
            for name in [
                &b"nvmlDeviceGetComputeRunningProcesses_v3\0"[..],
                b"nvmlDeviceGetGraphicsRunningProcesses_v3\0",
            ] {
                let list: ProcessList = self.sym(name)?;
                for p in fetch(|n, buf| list(dev, n, buf)) {
                    let vram = match p.used_gpu_memory {
                        VALUE_NOT_AVAILABLE => 0,
                        v => v,
                    };
                    // A process using both CUDA and graphics is listed twice with the same total.
                    let e = procs.entry(p.pid).or_default();
                    e.vram_bytes = e.vram_bytes.max(vram);
                }
            }

            let utilization: unsafe extern "C" fn(
                Device,
                *mut UtilizationSample,
                *mut c_uint,
                c_ulonglong,
            ) -> Ret = self.sym(b"nvmlDeviceGetProcessUtilization\0")?;
            // Sample timestamps are CPU time in microseconds since the epoch.
            let since = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .saturating_sub(window)
                .as_micros() as c_ulonglong;
            let mut sums = BTreeMap::<u32, (u32, u32)>::new();
            for s in fetch(|n, buf| utilization(dev, buf, n, since)) {
                let e = sums.entry(s.pid).or_default();
                e.0 += s.sm_util;
                e.1 += 1;
            }
            for (pid, (sum, count)) in sums {
                procs.entry(pid).or_default().sm_percent = f64::from(sum) / f64::from(count);
            }

            let gpu = Gpu {
                vram_used_bytes: mem.used,
                vram_total_bytes: mem.total,
                utilization_percent,
                power_milliwatts,
                apps: BTreeMap::new(),
            };
            Some((gpu, procs))
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        if let Some(shutdown) = self.sym::<unsafe extern "C" fn() -> Ret>(b"nvmlShutdown\0") {
            unsafe { shutdown() };
        }
    }
}

/// Runs an NVML list query: ask for the count, then fill a buffer of that size. Any error
/// (including "no samples yet") yields an empty list.
fn fetch<T: Default + Clone>(mut call: impl FnMut(*mut c_uint, *mut T) -> Ret) -> Vec<T> {
    let mut n: c_uint = 0;
    if call(&mut n, std::ptr::null_mut()) != INSUFFICIENT_SIZE {
        return Vec::new(); // Success with a null buffer means the list is empty.
    }
    // Room for processes that start between the two calls.
    n += 8;
    let mut buf = vec![T::default(); n as usize];
    if call(&mut n, buf.as_mut_ptr()) != SUCCESS {
        return Vec::new();
    }
    buf.truncate(n as usize);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sebwatch-nvidia-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn pci(&self, addr: &str, vendor: &str, class: &str, status: &str) {
            self.write(&format!("{addr}/vendor"), &format!("{vendor}\n"));
            self.write(&format!("{addr}/class"), &format!("{class}\n"));
            self.write(
                &format!("{addr}/power/runtime_status"),
                &format!("{status}\n"),
            );
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_the_nvidia_gpu_and_its_pm_status() {
        let fx = Fixture::new("pci");
        fx.pci("0000:00:02.0", "0x8086", "0x030000", "active");
        // NVIDIA's HDMI audio function is not the GPU.
        fx.pci("0000:01:00.1", "0x10de", "0x040300", "active");
        assert_eq!(find_gpu(&fx.0), None);

        fx.pci("0000:01:00.0", "0x10de", "0x030200", "suspended");
        assert_eq!(find_gpu(&fx.0), Some(("0000:01:00.0".into(), true)));
        fx.pci("0000:01:00.0", "0x10de", "0x030200", "active");
        assert_eq!(find_gpu(&fx.0), Some(("0000:01:00.0".into(), false)));
    }

    #[test]
    fn sums_processes_per_cgroup() {
        let fx = Fixture::new("proc");
        let game =
            "user.slice/user-1000.slice/user@1000.service/app.slice/app-steam@autostart.service";
        fx.write("10/cgroup", &format!("0::/{game}\n"));
        fx.write("11/cgroup", &format!("0::/{game}\n"));
        fx.write("20/cgroup", "0::/user.slice/app-firefox-1.scope\n");
        // PID 30 exited after NVML listed it: no /proc entry.
        let procs = BTreeMap::from([
            (
                10,
                ProcUsage {
                    vram_bytes: 1000,
                    sm_percent: 40.0,
                },
            ),
            (
                11,
                ProcUsage {
                    vram_bytes: 500,
                    sm_percent: 5.5,
                },
            ),
            (
                20,
                ProcUsage {
                    vram_bytes: 200,
                    sm_percent: 0.0,
                },
            ),
            (
                30,
                ProcUsage {
                    vram_bytes: 999,
                    sm_percent: 99.0,
                },
            ),
        ]);
        let apps = per_app(&fx.0, &procs);
        assert_eq!(
            apps,
            BTreeMap::from([
                (
                    PathBuf::from("user.slice/app-firefox-1.scope"),
                    AppUsage {
                        vram_bytes: 200,
                        sm_percent: 0.0
                    }
                ),
                (
                    PathBuf::from(game),
                    AppUsage {
                        vram_bytes: 1500,
                        sm_percent: 45.5
                    }
                ),
            ])
        );
    }
}
