use std::process::Command;

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Client {
    pub pid: u32,
    pub class: String,
    pub title: String,
    pub rank: Option<i64>,
}

pub fn compositor() -> &'static str {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        "Hyprland"
    } else if std::env::var_os("SWAYSOCK").is_some() {
        "Sway"
    } else if std::env::var_os("NIRI_SOCKET").is_some() {
        "niri"
    } else {
        "your compositor"
    }
}

pub fn clients() -> Vec<Client> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return hyprland();
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return sway();
    }
    if std::env::var_os("NIRI_SOCKET").is_some() {
        return niri();
    }
    Vec::new()
}

fn json(program: &str, args: &[&str]) -> Option<Value> {
    let out = Command::new(program).args(args).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn hyprland() -> Vec<Client> {
    let Some(Value::Array(list)) = json("hyprctl", &["-j", "clients"]) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            Some(Client {
                pid: c.get("pid")?.as_i64()?.try_into().ok()?,
                class: text(c, "class"),
                title: text(c, "title"),
                rank: c.get("focusHistoryID").and_then(Value::as_i64),
            })
        })
        .collect()
}

fn sway() -> Vec<Client> {
    let Some(tree) = json("swaymsg", &["-t", "get_tree", "-r"]) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    collect_sway(&tree, &mut found);
    found
}

fn collect_sway(node: &Value, found: &mut Vec<Client>) {
    if let Some(pid) = node.get("pid").and_then(Value::as_i64) {
        let class = node
            .get("app_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                node.get("window_properties")
                    .and_then(|p| p.get("class"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if let Ok(pid) = pid.try_into() {
            found.push(Client {
                pid,
                class,
                title: text(node, "name"),
                rank: None,
            });
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(Value::Array(children)) = node.get(key) {
            for child in children {
                collect_sway(child, found);
            }
        }
    }
}

fn niri() -> Vec<Client> {
    let Some(Value::Array(list)) = json("niri", &["msg", "-j", "windows"]) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|w| {
            Some(Client {
                pid: w.get("pid")?.as_i64()?.try_into().ok()?,
                class: text(w, "app_id"),
                title: text(w, "title"),
                rank: None,
            })
        })
        .collect()
}

pub fn find<'a>(clients: &'a [Client], app_id: &str, title: &str) -> Option<&'a Client> {
    let same_app = |c: &&Client| c.class.eq_ignore_ascii_case(app_id);
    clients
        .iter()
        .filter(same_app)
        .find(|c| c.title == title)
        .or_else(|| {
            let mut matches = clients.iter().filter(same_app);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
}
