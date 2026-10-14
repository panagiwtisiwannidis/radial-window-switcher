use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, ToolTip, Tray};

use crate::settings;

pub enum Command {
    Quit,
}

struct RadialTray {
    commands: Sender<Command>,
    hotkey: String,
    icons: Vec<Icon>,
}

impl Tray for RadialTray {
    fn id(&self) -> String {
        "radial-switcher".into()
    }

    fn title(&self) -> String {
        "Radial Switcher".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Radial Switcher".into(),
            description: format!("Hold {} to switch windows", self.hotkey),
            ..ToolTip::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: format!("Hold {} to switch windows", self.hotkey),
                enabled: false,
                ..StandardItem::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Start at login".into(),
                checked: settings::autostart_enabled(),
                activate: Box::new(|_: &mut Self| {
                    settings::set_autostart(!settings::autostart_enabled())
                }),
                ..CheckmarkItem::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Radial Switcher".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.commands.send(Command::Quit);
                }),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

fn icon(size: u32) -> Icon {
    let pixmap = radial_core::icon::app_icon(size);
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for p in pixmap.data().chunks(4) {
        let a = p[3] as u32;
        let straight = |c: u8| {
            (c as u32 * 255)
                .checked_div(a)
                .map_or(0, |v| v.min(255) as u8)
        };
        data.extend_from_slice(&[p[3], straight(p[0]), straight(p[1]), straight(p[2])]);
    }
    Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

pub fn start(commands: Sender<Command>, hotkey: &str) -> Option<Handle<impl Tray>> {
    let tray = RadialTray {
        commands,
        hotkey: hotkey.to_string(),
        icons: vec![icon(22), icon(32), icon(48), icon(64)],
    };
    tray.spawn().ok()
}
