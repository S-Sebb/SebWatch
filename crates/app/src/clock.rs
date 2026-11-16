//! The local time of day, for saying when the reading shown was taken.

/// Now as local "HH:MM:SS".
pub fn now() -> String {
    let mut seconds: libc::time_t = 0;
    // SAFETY: `time` and `localtime_r` only write to the values passed in.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::time(&mut seconds);
        libc::localtime_r(&seconds, &mut tm);
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
