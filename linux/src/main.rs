mod app;
mod capture;
mod desktop;
mod hotkey;
mod ring;
mod settings;
mod snapshots;
mod surface;
mod tabs;
mod tray;
mod windows;
mod x;

mod wayland;

fn wayland_session() -> bool {
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let session_wayland =
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
    let no_x = std::env::var_os("DISPLAY").is_none();
    has_wayland
        && (session_wayland || no_x)
        && std::env::var_os("RADIAL_SWITCHER_FORCE_X11").is_none()
}

fn main() {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        Some("show") => wayland::send("show"),
        Some(other) => Err(format!(
            "unknown command `{other}` (use `radial-switcher` to run it, or `radial-switcher show` to open the ring on Wayland)"
        )),
        None if wayland_session() => wayland::run(),
        None => app::run(),
    };
    if let Err(error) = result {
        eprintln!("Radial Switcher: {error}");
        std::process::exit(1);
    }
}
