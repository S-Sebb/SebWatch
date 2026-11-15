//! Prints one NVIDIA reading, for checking the NVML module on a real PC:
//! `cargo run -p sebwatch-core --example nvidia`.

fn main() {
    println!(
        "{:#?}",
        sebwatch_core::nvidia::read(std::time::Duration::from_secs(1))
    );
}
