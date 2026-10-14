# Radial Switcher for Windows

The Windows version of Radial Switcher. Hold **Alt+Space** and a ring opens around the cursor: the
windows of the app you're in on the inside, every other app on the outside. Aim with the mouse (or press
Tab), let go, and that window comes to the front.

> **Status: early.** This version has been built and unit-tested on CI, but not yet tried on a real
> Windows PC. Please report anything that doesn't work.

## Features

- Two rings: inner = windows of the foreground app, outer = all other apps (Microsoft Store apps
  included).
- **Window arc**: rest on an app with several windows and they fan out behind it, with thumbnails.
- **Tab arc**: go on to a window (or rest on a single-window app) and its tabs fan out behind it. For the
  app you're in, pause on one of its windows and its tabs pop up over the faded app ring. Works with
  Edge, Chrome, Firefox, Brave, Opera, Vivaldi, Windows Terminal, File Explorer, Notepad and VS Code.
- Large **live** preview card of the selected window (DWM thumbnails: video keeps playing). Background
  tabs and minimized windows show their last snapshot.
- Glass look (blurred background, rim light), opening and hover animations.
- Angle-based aiming, dead zone in the middle; only what's under the cursor is ever opened.
- Close button: the item you point at shows an × in its corner; click it to quit the app, close the
  window or close the tab. The ring stays open and updates.
- Keyboard: Tab / Shift+Tab, Enter, Esc. Right-click or click away cancels.
- Follows the Windows light/dark theme and accent color; works on high-DPI and multi-monitor setups;
  near screen edges the arcs rotate to stay on screen.
- Tray icon with **Start with Windows** and **Quit**.

Snapshots for thumbnails are taken every few seconds while windows are visible and kept in memory only.

## Requirements

Windows 10 (1903) or later, 64-bit. Windows 11 gives the preview card rounded corners.

## Get it

**Prebuilt**: in the GitHub repository go to **Actions → Windows build**, open the latest successful run
and download the **RadialSwitcher-windows** artifact (a zip with `RadialSwitcher.exe`).

**From source** (needs [Rust](https://rustup.rs) and the Visual Studio C++ Build Tools):

```powershell
cd windows
cargo build --release
.\target\release\RadialSwitcher.exe
```

There's no installer: put `RadialSwitcher.exe` anywhere (for example `%LOCALAPPDATA%\RadialSwitcher`)
and run it. Windows SmartScreen may warn the first time because the executable isn't code-signed: choose
**More info → Run anyway**.

## Usage

| Action | How |
|---|---|
| Open the ring | Hold **Alt+Space** |
| Pick a window / app | Move the cursor toward it, then release (or click) |
| Another app's windows | Rest on the app in the outer ring, keep going outward onto the window arc |
| A tab of another app | From the window arc go further out onto its tab arc |
| Tabs of the current app | Pause briefly on one of its windows in the inner ring |
| Close an app, window or tab | Point at it and click the **×** in its corner (an app quits; the ring stays open) |
| Keyboard | **Tab** / **Shift+Tab** to move, **Enter** to open, **Esc** to cancel |
| Cancel | Esc, right-click, or click away from the ring |

If you release Alt+Space with nothing selected, the ring stays open: pick with a click, Tab + Enter, or
dismiss with Esc.

While Radial Switcher runs it owns **Alt+Space**, so the Windows window menu (normally on Alt+Space) isn't
available from the keyboard; it's still on right-click of a title bar.

## Start with Windows / Quit

Right-click (or click) the tray icon: **Start with Windows** toggles starting at sign-in (stored in
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`), **Quit Radial Switcher** exits.

Some apps running as administrator can't be switched to from a normal (non-elevated) Radial Switcher; run
it elevated if you need that.

## Development

```bash
cd windows
cargo test                        # ring geometry and model tests (any OS)
cargo run --example preview -- .  # render sample rings to PNG (any OS)
cargo run --example make_ico      # regenerate assets/app.ico
cargo check --target x86_64-pc-windows-msvc   # type-check the Windows code from macOS/Linux
```

| Path | What |
|---|---|
| `src/geometry.rs`, `src/model.rs` | Ring layout and selection logic, shared with future ports |
| `src/render.rs`, `src/text.rs`, `src/icon.rs` | Drawing the ring (tiny-skia), text and the app icon |
| `src/platform/` | Windows integration: hooks, window list, activation, overlay, preview, tray, snapshots, UI Automation tabs |

## License

Copyright © 2026 Panagiotis Ioannidis. All rights reserved. See [LICENSE](../LICENSE).
