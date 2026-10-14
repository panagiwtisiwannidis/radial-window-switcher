#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    radial_switcher::platform::app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Radial Switcher for Windows only runs on Windows. See ../core for the shared ring code and its preview example.");
}
