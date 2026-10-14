# Radial Switcher for Linux

The Linux version of Radial Switcher. Hold **Alt+Space** and a ring opens around the cursor: the windows
of the app you're in on the inside, every other app on the outside. Aim with the mouse (or press Tab),
let go, and that window comes to the front.

> **Status: early.** Built, linted and smoke-tested on CI (a virtual X11 desktop opens the ring and
> switches windows), not yet tried on a real desktop. Please report anything that doesn't work.

## Requirements

- An **X11 session**. Wayland doesn't let apps list, preview or focus other apps' windows, so Radial
  Switcher refuses to start there. On Ubuntu/GNOME pick **"Ubuntu on Xorg"** (gear icon on the login
  screen); KDE has **"Plasma (X11)"**.
- A window manager that follows EWMH (GNOME, KDE, Xfce, Cinnamon, MATE, Openbox, i3, …).
- A compositor for transparency (built into GNOME, KDE, Cinnamon; Xfce: enable it in Window Manager
  Tweaks). Without one the ring still works but has a solid background.
- For the tray icon: KDE, or GNOME with the AppIndicator extension (preinstalled on Ubuntu).

## Features

- Two rings: inner = windows of the focused app, outer = all other apps.
- Window arc: rest on an app with several windows and they fan out behind it, with thumbnails.
- Large **live** preview card of the selected window; minimized windows show their last snapshot.
- Glass look, opening and hover animations, angle-based aiming with a dead zone.
- Keyboard: Tab / Shift+Tab, Enter, Esc. Right-click or click away cancels.
- Works across multiple monitors and HiDPI (`Xft.dpi`); arcs rotate to stay on screen near edges.
- Tray icon with **Start at login** and **Quit**.

Not yet on Linux: browser tab arcs (needs AT-SPI accessibility; planned).

## Get it

**Prebuilt**: in the GitHub repository go to **Actions → Linux build**, open the latest successful run and
download **radial-switcher-linux** (a zip with the `radial-switcher` binary, x86-64).

```bash
unzip radial-switcher-linux.zip
chmod +x radial-switcher
mkdir -p ~/.local/bin && mv radial-switcher ~/.local/bin/
radial-switcher &
```

**From source** (needs [Rust](https://rustup.rs); no system libraries required):

```bash
cd linux
cargo build --release
./target/release/radial-switcher &
```

## Hotkey

Radial Switcher tries **Alt+Space** first. Many desktops already use it (GNOME and Xfce: window menu;
KDE: KRunner); then it falls back to **Ctrl+Alt+Space**, then **Super+Alt+Space**, and tells you which one it
got on start-up and in the tray menu. To get Alt+Space on GNOME, free it first:

```bash
gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
```

## Usage

| Action | How |
|---|---|
| Open the ring | Hold the hotkey (Alt+Space) |
| Pick a window / app | Move the cursor toward it, then release (or click) |
| Another app's windows | Rest on the app in the outer ring, keep going outward onto the window arc |
| Keyboard | **Tab** / **Shift+Tab** to move, **Enter** to open, **Esc** to cancel |
| Cancel | Esc, right-click, or click away from the ring |

If you release the hotkey with nothing selected, the ring stays open: pick with a click, Tab + Enter, or
dismiss with Esc.

## Start at login / Quit

Tray icon → **Start at login** writes `~/.config/autostart/radial-switcher.desktop`; **Quit Radial
Switcher** exits. Without a tray, start it from your session's startup applications and quit with
`pkill radial-switcher`.

## Troubleshooting

Run with `RADIAL_SWITCHER_DEBUG=1 radial-switcher` to log key events, grabs, selections and activation
requests to the terminal.

## Development

```bash
cd linux
cargo clippy --all-targets -- -D warnings
cargo check --target x86_64-unknown-linux-gnu   # from macOS
```

| Path | What |
|---|---|
| `src/app.rs` | Event loop, ring state, activation |
| `src/windows.rs`, `src/x.rs` | EWMH window list, app names (`.desktop` files) and icons |
| `src/capture.rs`, `src/snapshots.rs` | Window capture through Composite + Render, background snapshots |
| `src/surface.rs` | Transparent override-redirect windows for the ring and the preview card |
| `src/hotkey.rs`, `src/tray.rs`, `src/settings.rs` | Key grabs, tray icon, autostart, theme and scale |

The ring itself (layout, selection, drawing) is shared with the Windows version in [`../core`](../core).
