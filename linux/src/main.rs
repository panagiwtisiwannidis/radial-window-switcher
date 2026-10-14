mod app;
mod capture;
mod hotkey;
mod ring;
mod settings;
mod snapshots;
mod surface;
mod tabs;
mod tray;
mod windows;
mod x;

fn wayland_only() -> bool {
    let session_wayland =
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
    let no_x = std::env::var_os("DISPLAY").is_none();
    (session_wayland || no_x) && std::env::var_os("RADIAL_SWITCHER_FORCE_X11").is_none()
}

fn main() {
    if wayland_only() {
        eprintln!(
            "Radial Switcher needs an X11 session: Wayland doesn't let apps list, preview or focus other apps' windows.\n\
             Log out and pick an X11/Xorg session (e.g. \"Ubuntu on Xorg\") from the gear on the login screen."
        );
        std::process::exit(1);
    }
    if let Err(error) = app::run() {
        eprintln!("Radial Switcher: {error}");
        std::process::exit(1);
    }
}
