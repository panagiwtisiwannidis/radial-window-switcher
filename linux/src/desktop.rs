use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tiny_skia::Pixmap;

const ICON_SIZE: u32 = 128;
const ICON_SIZES: &[&str] = &[
    "256x256", "128x128", "96x96", "64x64", "48x48", "scalable", "512x512", "32x32",
];

#[derive(Clone)]
struct Entry {
    name: String,
    icon: Option<String>,
}

pub struct DesktopApps {
    entries: HashMap<String, Entry>,
    themes: Vec<String>,
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop"));
    dirs
}

impl DesktopApps {
    pub fn load() -> Self {
        let mut entries = HashMap::new();
        for dir in data_dirs() {
            let Ok(files) = std::fs::read_dir(dir.join("applications")) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let (mut name, mut icon, mut wm_class, mut in_entry) = (None, None, None, false);
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        in_entry = line == "[Desktop Entry]";
                        continue;
                    }
                    if !in_entry {
                        continue;
                    }
                    if let Some(value) = line.strip_prefix("Name=") {
                        name.get_or_insert_with(|| value.to_string());
                    } else if let Some(value) = line.strip_prefix("Icon=") {
                        icon.get_or_insert_with(|| value.to_string());
                    } else if let Some(value) = line.strip_prefix("StartupWMClass=") {
                        wm_class = Some(value.to_lowercase());
                    }
                }
                let Some(name) = name else { continue };
                let entry = Entry { name, icon };
                if let Some(class) = wm_class {
                    entries.entry(class).or_insert_with(|| entry.clone());
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let stem = stem.to_lowercase();
                    let short = stem.rsplit('.').next().unwrap_or(&stem).to_string();
                    entries.entry(short).or_insert_with(|| entry.clone());
                    entries.entry(stem).or_insert(entry);
                }
            }
        }
        Self {
            entries,
            themes: icon_themes(),
        }
    }

    fn entry(&self, keys: &[&str]) -> Option<&Entry> {
        keys.iter().filter(|key| !key.is_empty()).find_map(|key| {
            let key = key.to_lowercase();
            self.entries
                .get(&key)
                .or_else(|| self.entries.get(key.rsplit('.').next().unwrap_or(&key)))
        })
    }

    pub fn name(&self, keys: &[&str]) -> Option<String> {
        self.entry(keys).map(|entry| entry.name.clone())
    }

    pub fn icon(&self, keys: &[&str]) -> Option<Pixmap> {
        let named = self.entry(keys).and_then(|entry| entry.icon.clone());
        let candidates = named.into_iter().chain(
            keys.iter()
                .filter(|k| !k.is_empty())
                .map(|k| k.to_lowercase()),
        );
        for icon in candidates {
            if let Some(pixmap) = self.find_icon(&icon) {
                return Some(pixmap);
            }
        }
        None
    }

    fn find_icon(&self, icon: &str) -> Option<Pixmap> {
        if icon.starts_with('/') {
            return load_icon(Path::new(icon));
        }
        let bases: Vec<PathBuf> = data_dirs()
            .into_iter()
            .map(|d| d.join("icons"))
            .chain(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".icons")))
            .collect();
        for theme in &self.themes {
            for base in &bases {
                for size in ICON_SIZES {
                    for ext in ["png", "svg"] {
                        let path = base
                            .join(theme)
                            .join(size)
                            .join("apps")
                            .join(format!("{icon}.{ext}"));
                        if let Some(pixmap) = load_icon(&path) {
                            return Some(pixmap);
                        }
                    }
                }
            }
        }
        for dir in data_dirs() {
            for ext in ["png", "svg"] {
                if let Some(pixmap) = load_icon(&dir.join("pixmaps").join(format!("{icon}.{ext}")))
                {
                    return Some(pixmap);
                }
            }
        }
        None
    }
}

fn icon_themes() -> Vec<String> {
    let mut themes = Vec::new();
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
    {
        let theme = String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_matches('\'')
            .to_string();
        if !theme.is_empty() {
            themes.push(theme);
        }
    }
    for theme in ["hicolor", "Adwaita", "breeze", "Papirus"] {
        if !themes.iter().any(|t| t == theme) {
            themes.push(theme.to_string());
        }
    }
    themes
}

fn load_icon(path: &Path) -> Option<Pixmap> {
    if !path.is_file() {
        return None;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => Pixmap::load_png(path).ok(),
        Some("svg") => render_svg(path),
        _ => None,
    }
}

fn render_svg(path: &Path) -> Option<Pixmap> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = ICON_SIZE as f32 / size.width().max(size.height());
    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE)?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap)
}
