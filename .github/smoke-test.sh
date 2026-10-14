export DISPLAY=:99
export GNOME_ACCESSIBILITY=1
export NO_AT_BRIDGE=0
mkdir -p shots

Xvfb :99 -screen 0 1600x1000x24 +extension Composite +extension RANDR &
sleep 2
xsetroot -solid "#3b5b8a"
/usr/libexec/at-spi-bus-launcher --launch-immediately &
sleep 1
gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
  --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "<true>" || true

RADIAL_SWITCHER_DEBUG=1 ./linux/target/release/radial-switcher > shots/app.log 2>&1 &
sleep 2
openbox &
sleep 1
xcompmgr &

echo "=== Window switching"
xclock -geometry 200x200+1300+60 &
xterm -T "Second terminal" -geometry 90x30+700+300 -e sh -c 'echo SECOND; sleep 900' &
sleep 1
xterm -T "First terminal" -geometry 90x30+80+80 -e sh -c 'echo FIRST; sleep 900' &
sleep 3
xdotool search --name "First terminal" windowactivate --sync
sleep 1
before=$(xdotool getactivewindow getwindowname)
xdotool mousemove 800 520
sleep 0.5
xdotool keydown alt keydown space
sleep 1.5
import -window root shots/1-ring-open.png
xdotool mousemove 800 610
sleep 1
import -window root shots/2-other-window-selected.png
xdotool keyup space keyup alt
sleep 1.5
after=$(xdotool getactivewindow getwindowname)
echo "window switch: $before -> $after" | tee shots/result.txt
test "$before" = "First terminal"
test "$after" = "Second terminal"

echo "=== Close a window with its x button"
xdotool mousemove 800 520
sleep 0.5
xdotool keydown alt keydown space
sleep 1.5
xdotool mousemove 800 600
sleep 0.3
xdotool mousemove 800 640
sleep 1
import -window root shots/2b-close-window-hover.png
xdotool click 1
sleep 0.3
xdotool keyup space keyup alt
sleep 2
import -window root shots/2c-after-close-window.png
xdotool key Escape
sleep 1
if xdotool search --name "First terminal" > /dev/null 2>&1; then
  echo "window was not closed" | tee -a shots/result.txt
  exit 1
fi
echo "window close: First terminal is gone" | tee -a shots/result.txt

echo "=== Browser tabs"
profile=$(mktemp -d)
cat > "$profile/user.js" <<'PREFS'
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("startup.homepage_welcome_url", "");
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.startup.page", 0);
PREFS
pages=$(mktemp -d)
for name in Alpha Beta Gamma; do
  printf '<!doctype html><title>%s tab</title><h1>%s</h1>' "$name" "$name" > "$pages/$name.html"
done
firefox --new-instance --profile "$profile" -width 900 -height 650 \
  "file://$pages/Alpha.html" "file://$pages/Beta.html" "file://$pages/Gamma.html" > shots/firefox.log 2>&1 &
for i in $(seq 1 60); do
  xdotool search --name "Mozilla Firefox" > /dev/null 2>&1 && break
  sleep 1
done
sleep 8
xdotool search --name "Mozilla Firefox" | head -1 | xargs -I{} xdotool windowactivate --sync {}
sleep 1
xdotool key ctrl+1
sleep 1
tab_before=$(xdotool getactivewindow getwindowname)
xdotool mousemove 800 520
sleep 0.5
xdotool keydown alt keydown space
sleep 2.5
xdotool mousemove 800 445
sleep 0.2
xdotool mousemove 800 430
sleep 1.2
import -window root shots/3-tab-popup.png
for point in "830 360" "860 310" "880 285" "894 270"; do
  xdotool mousemove $point
  sleep 0.15
done
sleep 1
import -window root shots/4-tab-selected.png
xdotool keyup space keyup alt
sleep 2
import -window root shots/5-after-tab-switch.png
tab_after=$(xdotool getactivewindow getwindowname)
echo "tab switch: $tab_before -> $tab_after" | tee -a shots/result.txt
case "$tab_before" in Alpha*) ;; *) echo "unexpected starting tab"; exit 1 ;; esac
case "$tab_after" in Gamma*) ;; *) echo "tab did not switch"; exit 1 ;; esac

echo "=== Close a background tab with its x button"
xdotool mousemove 800 520
sleep 0.5
xdotool keydown alt keydown space
sleep 2.5
xdotool mousemove 800 445
sleep 0.2
xdotool mousemove 800 430
sleep 1.2
for y in 380 320 260 220 194; do
  xdotool mousemove 800 $y
  sleep 0.15
done
sleep 1
import -window root shots/6-close-tab-hover.png
xdotool click 1
sleep 0.3
xdotool keyup space keyup alt
sleep 3
import -window root shots/7-after-close-tab.png
xdotool key Escape
sleep 1
cat shots/app.log
grep -q 'close tab 1 of .*Ok(())' shots/app.log
last_tabs=$(grep 'tabs: \[' shots/app.log | tail -1)
echo "tabs after closing: $last_tabs" | tee -a shots/result.txt
case "$last_tabs" in *'["Alpha tab", "Gamma tab"]'*) ;; *) echo "tab was not closed"; exit 1 ;; esac
