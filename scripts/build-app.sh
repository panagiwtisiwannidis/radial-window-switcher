#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --install) INSTALL=1 ;;
        *) echo "Usage: scripts/build-app.sh [--install]" >&2; exit 2 ;;
    esac
done

APP_NAME="RadialSwitcher"
APP="build/${APP_NAME}.app"

echo "==> Building release binary"
swift build -c release
BIN_DIR="$(swift build -c release --show-bin-path)"

echo "==> Assembling ${APP}"
STAGE_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGE_DIR"' EXIT
STAGED="$STAGE_DIR/${APP_NAME}.app"
mkdir -p "$STAGED/Contents/MacOS" "$STAGED/Contents/Resources"
cp "$BIN_DIR/$APP_NAME" "$STAGED/Contents/MacOS/$APP_NAME"
cp Support/Info.plist "$STAGED/Contents/Info.plist"
cp Support/AppIcon.icns "$STAGED/Contents/Resources/AppIcon.icns"

IDENTITY="${SIGN_IDENTITY:-}"
if [[ -z "$IDENTITY" ]]; then
    IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
        | awk -F'"' '/Developer ID Application|Apple Development/ { print $2; exit }')"
fi
if [[ -z "$IDENTITY" ]]; then
    IDENTITY="-"
    echo "==> No signing certificate found: signing ad-hoc (permissions must be re-granted after each rebuild)"
else
    echo "==> Signing with: $IDENTITY"
fi
xattr -cr "$STAGED"
codesign --force --sign "$IDENTITY" "$STAGED"
codesign --verify --strict "$STAGED"

mkdir -p build
rm -rf "$APP"
ditto "$STAGED" "$APP"

if [[ "$INSTALL" == 1 ]]; then
    DEST="/Applications/${APP_NAME}.app"
    echo "==> Installing to ${DEST}"
    osascript -e "tell application id \"io.github.panagiwtisiwannidis.RadialSwitcher\" to quit" >/dev/null 2>&1 || true
    pkill -x "$APP_NAME" >/dev/null 2>&1 || true
    sleep 1
    rm -rf "$DEST"
    ditto "$STAGED" "$DEST"
    open "$DEST"
    echo "==> Done: Radial Switcher is running (look for its icon in the menu bar)"
else
    echo "==> Done: ${APP}"
fi
