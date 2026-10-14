use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub enum Request {
    Show,
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    let display = display
        .rsplit('/')
        .next()
        .unwrap_or("wayland-0")
        .to_string();
    runtime.join(format!("radial-switcher-{display}.sock"))
}

pub fn send(command: &str) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket_path()).map_err(|_| {
        "Radial Switcher isn't running (start it with `radial-switcher` first)".to_string()
    })?;
    stream
        .write_all(format!("{command}\n").as_bytes())
        .map_err(|e| e.to_string())
}

pub fn listen(handler: impl Fn(Request) + Send + 'static) -> Result<(), String> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err("Radial Switcher is already running".into());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| format!("can't listen on {}: {e}", path.display()))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok() && line.trim() == "show" {
                handler(Request::Show);
            }
        }
    });
    Ok(())
}
