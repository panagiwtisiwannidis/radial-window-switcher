use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window};

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        UTF8_STRING,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_ACTIVE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_WORKAREA,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_ICON,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _RADIAL_SWITCHER_RUNNING,
    }
}

pub fn values32(
    conn: &impl Connection,
    window: Window,
    property: Atom,
    kind: impl Into<Atom>,
) -> Vec<u32> {
    conn.get_property(false, window, property, kind, 0, u32::MAX / 4)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(|values| values.collect()))
        .unwrap_or_default()
}

pub fn bytes(
    conn: &impl Connection,
    window: Window,
    property: Atom,
    kind: impl Into<Atom>,
) -> Vec<u8> {
    conn.get_property(false, window, property, kind, 0, u32::MAX / 4)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.value)
        .unwrap_or_default()
}

pub fn text(conn: &impl Connection, window: Window, atoms: &Atoms) -> String {
    let utf8 = bytes(conn, window, atoms._NET_WM_NAME, atoms.UTF8_STRING);
    if !utf8.is_empty() {
        return String::from_utf8_lossy(&utf8).into_owned();
    }
    String::from_utf8_lossy(&bytes(
        conn,
        window,
        AtomEnum::WM_NAME.into(),
        AtomEnum::STRING,
    ))
    .into_owned()
}

pub fn class(conn: &impl Connection, window: Window) -> (String, String) {
    let raw = bytes(conn, window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING);
    let mut parts = raw
        .split(|&b| b == 0)
        .map(|p| String::from_utf8_lossy(p).into_owned());
    let instance = parts.next().unwrap_or_default();
    let class = parts.next().unwrap_or_default();
    (instance, class)
}
