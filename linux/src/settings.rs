use std::path::PathBuf;
use std::process::Command;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, Window};

use crate::x;

const AUTOSTART_FILE: &str = "radial-switcher.desktop";

fn autostart_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join(AUTOSTART_FILE))
}

pub fn autostart_enabled() -> bool {
    autostart_path().is_some_and(|path| path.exists())
}

pub fn set_autostart(enabled: bool) {
    let Some(path) = autostart_path() else { return };
    if !enabled {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Radial Switcher\nComment=Radial window switcher\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
        exe.display()
    );
    let _ = std::fs::write(path, entry);
}

pub fn dark_mode() -> bool {
    let read = |schema: &str, key: &str| {
        Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).to_lowercase())
    };
    if let Some(scheme) = read("org.gnome.desktop.interface", "color-scheme") {
        if scheme.contains("dark") {
            return true;
        }
        if scheme.contains("light") {
            return false;
        }
    }
    match read("org.gnome.desktop.interface", "gtk-theme") {
        Some(theme) if !theme.trim().is_empty() => theme.contains("dark"),
        _ => true,
    }
}

pub fn scale(conn: &impl Connection, root: Window) -> f32 {
    let resources = x::bytes(
        conn,
        root,
        AtomEnum::RESOURCE_MANAGER.into(),
        AtomEnum::STRING,
    );
    let resources = String::from_utf8_lossy(&resources);
    let dpi = resources
        .lines()
        .find_map(|line| {
            line.strip_prefix("Xft.dpi:")
                .map(|v| v.trim().parse::<f32>().ok())
        })
        .flatten()
        .unwrap_or(96.0);
    (dpi / 96.0).clamp(1.0, 4.0)
}

pub fn font_path() -> Option<String> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let supported = path.ends_with(".ttf") || path.ends_with(".otf");
    (supported && std::path::Path::new(&path).exists()).then_some(path)
}
