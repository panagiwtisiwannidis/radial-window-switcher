A radial window switcher for macOS, Windows and Linux. Hold **Alt+Space** (⌥Space on the Mac), aim at a
window, app or browser tab, and let go.

## Downloads

| Platform | File | How to run |
|---|---|---|
| **macOS** 14+ (Apple Silicon and Intel) | `RadialSwitcher-…-macos.zip` | Unzip, move **Radial Switcher** to Applications. The app isn't notarized: the first time, right-click it → **Open** → **Open** (or run `xattr -dr com.apple.quarantine /Applications/RadialSwitcher.app`). Then grant Accessibility and Screen Recording when asked. |
| **Windows** 10/11 (x64) | `RadialSwitcher-…-windows-x64.zip` | Unzip and run `RadialSwitcher.exe`. SmartScreen may warn about an unknown publisher: **More info → Run anyway**. |
| **Linux** x86-64 (X11, or Wayland on Hyprland, Sway, niri, …) | `radial-switcher-…-linux-x86_64.tar.gz` | `tar -xzf radial-switcher-*.tar.gz && ./radial-switcher &`. On Wayland, bind a key to `radial-switcher show` ([setup](https://github.com/panagiwtisiwannidis/radial-window-switcher/blob/main/linux/README.md#wayland)). |

See the [README](https://github.com/panagiwtisiwannidis/radial-window-switcher#readme) for features and usage.
