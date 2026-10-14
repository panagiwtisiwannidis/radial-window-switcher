use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as XkbExt};
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, Keycode, ModMask, Window};

const XK_SPACE: u32 = 0x0020;
const XK_TAB: u32 = 0xff09;
const XK_ISO_LEFT_TAB: u32 = 0xfe20;
const XK_RETURN: u32 = 0xff0d;
const XK_KP_ENTER: u32 = 0xff8d;
const XK_ESCAPE: u32 = 0xff1b;

pub struct Keys {
    pub space: Keycode,
    pub tab: Vec<Keycode>,
    pub enter: Vec<Keycode>,
    pub escape: Vec<Keycode>,
}

impl Keys {
    pub fn load(conn: &impl Connection) -> Option<Self> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()?
            .reply()
            .ok()?;
        let per = mapping.keysyms_per_keycode as usize;
        let find = |keysym: u32| -> Vec<Keycode> {
            mapping
                .keysyms
                .chunks(per.max(1))
                .enumerate()
                .filter(|(_, syms)| syms.contains(&keysym))
                .map(|(i, _)| min + i as u8)
                .collect()
        };
        let space = *find(XK_SPACE).first()?;
        let mut tab = find(XK_TAB);
        tab.extend(find(XK_ISO_LEFT_TAB));
        let mut enter = find(XK_RETURN);
        enter.extend(find(XK_KP_ENTER));
        Some(Self {
            space,
            tab,
            enter,
            escape: find(XK_ESCAPE),
        })
    }
}

pub fn enable_detectable_autorepeat(conn: &impl Connection) {
    if conn
        .xkb_use_extension(1, 0)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|r| r.supported)
    {
        let flag = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
        let _ = conn
            .xkb_per_client_flags(
                xkb::ID::USE_CORE_KBD.into(),
                flag,
                flag,
                0u32.into(),
                0u32.into(),
                0u32.into(),
            )
            .map(|c| c.reply());
    }
}

fn grab(conn: &impl Connection, root: Window, key: Keycode, modifiers: ModMask) -> bool {
    let lock_variants = [
        ModMask::from(0u16),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ];
    let mut ok = true;
    for extra in lock_variants {
        let result = conn.grab_key(
            false,
            root,
            modifiers | extra,
            key,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        );
        ok &= result.map(|c| c.check().is_ok()).unwrap_or(false);
    }
    if !ok {
        for extra in lock_variants {
            let _ = conn.ungrab_key(key, root, modifiers | extra);
        }
    }
    ok
}

pub fn register(conn: &impl Connection, root: Window, keys: &Keys) -> Option<&'static str> {
    let candidates = [
        (ModMask::M1, "Alt+Space"),
        (ModMask::CONTROL | ModMask::M1, "Ctrl+Alt+Space"),
        (ModMask::M4 | ModMask::M1, "Super+Alt+Space"),
    ];
    candidates
        .into_iter()
        .find(|(modifiers, _)| grab(conn, root, keys.space, *modifiers))
        .map(|(_, label)| label)
}
