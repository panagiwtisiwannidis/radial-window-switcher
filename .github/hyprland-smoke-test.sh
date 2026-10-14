export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
unset DISPLAY
export XDG_SESSION_TYPE=wayland
mkdir -p shots/hyprland
BIN=./radial-switcher
chmod +x "$BIN"

cat > "$XDG_RUNTIME_DIR/hyprland.conf" <<'CONF'
monitor = , 1600x1000, 0x0, 1
animations {
    enabled = false
}
misc {
    disable_hyprland_logo = true
    disable_splash_rendering = true
}
decoration {
    rounding = 0
}
CONF

HYPRLAND_HEADLESS_ONLY=1 Hyprland --i-am-really-stupid -c "$XDG_RUNTIME_DIR/hyprland.conf" > shots/hyprland/hyprland.log 2>&1 &
for i in $(seq 1 100); do
  ls "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock > /dev/null 2>&1 && break
  sleep 0.2
done
export HYPRLAND_INSTANCE_SIGNATURE=$(basename "$(dirname "$(ls "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock | head -1)")")
export WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -1)")
echo "HYPRLAND_INSTANCE_SIGNATURE=$HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
hyprctl monitors | head -5 || true
if ! hyprctl monitors | grep -q Monitor; then
  hyprctl output create headless
  sleep 1
  hyprctl monitors | head -5
fi

focused() {
  hyprctl activewindow -j | jq -r .title
}

foot --title "Second terminal" sh -c 'echo SECOND; sleep 900' &
sleep 2
foot --title "First terminal" sh -c 'echo FIRST; sleep 900' &
sleep 2
hyprctl dispatch focuswindow 'title:^First terminal$'
sleep 0.5
hyprctl clients | head -30

RADIAL_SWITCHER_DEBUG=1 $BIN > shots/hyprland/app.log 2>&1 &
sleep 2
cat shots/hyprland/app.log

hyprctl dispatch movecursor 800 500
sleep 0.5
before=$(focused)
grim shots/hyprland/0-desktop.png || true

$BIN show
sleep 1.5
grim shots/hyprland/1-ring-open.png || true
hyprctl dispatch movecursor 800 545
sleep 0.2
hyprctl dispatch movecursor 800 590
sleep 1
grim shots/hyprland/2-other-window-selected.png || true
wtype -k Return
sleep 1.5
grim shots/hyprland/3-after-enter.png || true
after=$(focused)
echo "hyprland pointer + Enter: $before -> $after" | tee shots/hyprland/result.txt
cat shots/hyprland/app.log
test "$before" = "First terminal"
test "$after" = "Second terminal"
