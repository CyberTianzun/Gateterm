#!/usr/bin/env bash
# 把 gateterm-desktop-web 与 gateterm-desktop-native 组装成 macOS .app。
# gateterm-desktop-web 设置 LSUIElement=1(纯托盘常驻宿主,不在 Dock 留图标);
# gateterm-desktop-native 为完整 GPU 终端工作台窗口程序(保留 Dock 图标与菜单栏)。
set -euo pipefail

# 默认值跟随本机架构:写死 aarch64 会让 Intel Mac 上的报错建议指向错误的 target
case "$(uname -m)" in
  arm64) HOST_TARGET="aarch64-apple-darwin" ;;
  *)     HOST_TARGET="x86_64-apple-darwin" ;;
esac
TARGET="${1:-$HOST_TARGET}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICON="$ROOT/clients/gateterm-desktop-web/assets/icon.icns"

# 1. gateterm-desktop-web.app
WEB_APP="$ROOT/target/bundle/gateterm-desktop-web.app"
WEB_BIN="$ROOT/target/$TARGET/release/gateterm-desktop-web"
WEB_VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/clients/gateterm-desktop-web/Cargo.toml" | head -1)"
[ -n "$WEB_VERSION" ] || { echo "无法从 clients/gateterm-desktop-web/Cargo.toml 读出 version" >&2; exit 1; }
[ -f "$WEB_BIN" ] || { echo "找不到 $WEB_BIN,先跑 cargo build --release --target $TARGET -p gateterm-desktop-web" >&2; exit 1; }

rm -rf "$WEB_APP"
mkdir -p "$WEB_APP/Contents/MacOS" "$WEB_APP/Contents/Resources"
cp "$WEB_BIN" "$WEB_APP/Contents/MacOS/gateterm-desktop-web"
cp "$ICON" "$WEB_APP/Contents/Resources/icon.icns"

cat > "$WEB_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>gateterm-desktop-web</string>
    <key>CFBundleDisplayName</key>
    <string>Gateterm Desktop Web</string>
    <key>CFBundleIdentifier</key>
    <string>com.gateterm.desktop.web</string>
    <key>CFBundleExecutable</key>
    <string>gateterm-desktop-web</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$WEB_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
PLIST
echo "$WEB_APP"

# 2. gateterm-desktop-native.app
NATIVE_APP="$ROOT/target/bundle/gateterm-desktop-native.app"
NATIVE_BIN="$ROOT/clients/gateterm-desktop-native/target/$TARGET/release/gateterm-desktop-native"
NATIVE_VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/clients/gateterm-desktop-native/Cargo.toml" | head -1)"
[ -n "$NATIVE_VERSION" ] || { echo "无法从 clients/gateterm-desktop-native/Cargo.toml 读出 version" >&2; exit 1; }
[ -f "$NATIVE_BIN" ] || { echo "找不到 $NATIVE_BIN,先跑 cargo build --release --target $TARGET --manifest-path clients/gateterm-desktop-native/Cargo.toml -p gateterm-desktop-native" >&2; exit 1; }

rm -rf "$NATIVE_APP"
mkdir -p "$NATIVE_APP/Contents/MacOS" "$NATIVE_APP/Contents/Resources"
cp "$NATIVE_BIN" "$NATIVE_APP/Contents/MacOS/gateterm-desktop-native"
cp "$ICON" "$NATIVE_APP/Contents/Resources/icon.icns"

cat > "$NATIVE_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>gateterm-desktop-native</string>
    <key>CFBundleDisplayName</key>
    <string>Gateterm</string>
    <key>CFBundleIdentifier</key>
    <string>com.gateterm.desktop.native</string>
    <key>CFBundleExecutable</key>
    <string>gateterm-desktop-native</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$NATIVE_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
echo "$NATIVE_APP"
