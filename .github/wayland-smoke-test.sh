export XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
unset DISPLAY
export XDG_SESSION_TYPE=wayland
mkdir -p shots/wayland

cat > "$XDG_RUNTIME_DIR/sway.conf" <<'CONF'
output HEADLESS-1 resolution 1600x1000 position 0 0 bg #3b5b8a solid_color
default_border normal 2
focus_follows_mouse no
CONF

WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c "$XDG_RUNTIME_DIR/sway.conf" > shots/wayland/sway.log 2>&1 &
for i in $(seq 1 50); do
  ls "$XDG_RUNTIME_DIR"/wayland-* > /dev/null 2>&1 && break
  sleep 0.2
done
export WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -1)")
export SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY SWAYSOCK=$SWAYSOCK"
swaymsg -t get_outputs | head -5

focused() {
  swaymsg -t get_tree | jq -r '.. | objects | select(.focused == true) | .name'
}

foot --title "Second terminal" sh -c 'echo SECOND; sleep 900' &
sleep 2
foot --title "First terminal" sh -c 'echo FIRST; sleep 900' &
sleep 2
swaymsg '[title="First terminal"] focus'
sleep 0.5

RADIAL_SWITCHER_DEBUG=1 ./linux/target/release/radial-switcher > shots/wayland/app.log 2>&1 &
sleep 2
cat shots/wayland/app.log

wlrctl pointer move -3000 -3000
wlrctl pointer move 800 500
sleep 0.5
before=$(focused)
grim shots/wayland/0-desktop.png

./linux/target/release/radial-switcher show
sleep 1.5
grim shots/wayland/1-ring-open.png
wlrctl pointer move 0 45
sleep 0.2
wlrctl pointer move 0 45
sleep 1
grim shots/wayland/2-other-window-selected.png
wlrctl pointer click left
sleep 1.5
grim shots/wayland/3-after-click.png
after=$(focused)
if [ "$after" != "Second terminal" ]; then
  echo "click did not switch; trying Enter"
  wtype -k Return
  sleep 1.5
  grim shots/wayland/4-after-enter.png
  after=$(focused)
fi
echo "wayland window switch: $before -> $after" | tee shots/wayland/result.txt
cat shots/wayland/app.log
test "$before" = "First terminal"
test "$after" = "Second terminal"
