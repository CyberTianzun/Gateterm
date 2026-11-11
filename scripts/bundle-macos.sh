#!/usr/bin/env bash
# 把 gateterm-desktop-web 与 gateterm-desktop-native 组装成带签名的 macOS .app,
# 并分别打包为附带 /Applications 拖拽快捷方式的 .dmg 磁盘映像。
# - Gateterm Desktop Web.app:设置 LSUIElement=1(纯托盘常驻宿主,不在 Dock 留图标)
# - Gateterm.app:完整 GPU 终端工作台窗口程序(保留 Dock 图标与菜单栏)
set -euo pipefail

# 默认值跟随本机架构:写死 aarch64 会让 Intel Mac 上的报错建议指向错误的 target
case "$(uname -m)" in
  arm64) HOST_TARGET="aarch64-apple-darwin" ;;
  *)     HOST_TARGET="x86_64-apple-darwin" ;;
esac
TARGET="${1:-$HOST_TARGET}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICON="$ROOT/clients/desktop-web/assets/icon.icns"
BUNDLE_DIR="$ROOT/target/bundle"

STAGE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/gateterm-dmg.XXXXXX")"
trap 'rm -rf "$STAGE_ROOT"' EXIT

sign_app() {
  local app_path="$1"
  codesign --force --deep --sign - "$app_path"
  codesign --verify --deep --strict "$app_path"
}

build_dmg() {
  local app_path="$1"
  local vol_name="$2"
  local dmg_path="$3"
  local stage_name="$4"
  local app_basename
  app_basename="$(basename "$app_path")"
  local stage_dir="$STAGE_ROOT/$stage_name"

  rm -rf "$stage_dir"
  mkdir -p "$stage_dir"
  # ditto 保留 .app 的权限位、扩展属性与代码签名密封
  ditto "$app_path" "$stage_dir/$app_basename"
  ln -s /Applications "$stage_dir/Applications"

  rm -f "$dmg_path"
  # GitHub Actions macOS runner 上 Spotlight 偶发占用临时镜像报 Resource busy,最多重试 5 次
  local attempt
  for attempt in 1 2 3 4 5; do
    if hdiutil create -volname "$vol_name" -srcfolder "$stage_dir" -ov -format UDZO "$dmg_path"; then
      rm -rf "$stage_dir"
      return 0
    fi
    rm -f "$dmg_path"
    echo "hdiutil create 第 ${attempt} 次失败,等待后重试..." >&2
    sleep 2
  done
  echo "无法创建 DMG: $dmg_path" >&2
  return 1
}

mkdir -p "$BUNDLE_DIR"
# 清理历史遗留的旧命名目录,避免本地重复构建时混淆
rm -rf "$BUNDLE_DIR/gateterm-desktop-web.app" "$BUNDLE_DIR/gateterm-desktop-native.app"

# 1. Gateterm Desktop Web.app -> gateterm-desktop-web.dmg
WEB_APP="$BUNDLE_DIR/Gateterm Desktop Web.app"
WEB_DMG="$BUNDLE_DIR/gateterm-desktop-web.dmg"
WEB_BIN="$ROOT/target/$TARGET/release/gateterm-desktop-web"
WEB_VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" "$ROOT/clients/desktop-web/Cargo.toml" | head -1)"
[ -n "$WEB_VERSION" ] || { echo "无法从 Cargo.toml 读出 version" >&2; exit 1; }
[ -f "$WEB_BIN" ] || { echo "找不到 $WEB_BIN,先跑 cargo build --release --target $TARGET -p gateterm-desktop-web" >&2; exit 1; }

rm -rf "$WEB_APP" "$WEB_DMG"
mkdir -p "$WEB_APP/Contents/MacOS" "$WEB_APP/Contents/Resources"
cp "$WEB_BIN" "$WEB_APP/Contents/MacOS/gateterm-desktop-web"
chmod 755 "$WEB_APP/Contents/MacOS/gateterm-desktop-web"
cp "$ICON" "$WEB_APP/Contents/Resources/icon.icns"

cat > "$WEB_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Gateterm Desktop Web</string>
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
    <key>CFBundleVersion</key>
    <string>$WEB_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
PLIST

sign_app "$WEB_APP"
build_dmg "$WEB_APP" "Gateterm Desktop Web" "$WEB_DMG" "web"
echo "$WEB_APP"
echo "$WEB_DMG"

# 2. Gateterm.app -> gateterm-desktop-native.dmg
NATIVE_APP="$BUNDLE_DIR/Gateterm.app"
NATIVE_DMG="$BUNDLE_DIR/gateterm-desktop-native.dmg"
NATIVE_BIN="$ROOT/clients/desktop-native/target/$TARGET/release/gateterm-desktop-native"
NATIVE_VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/clients/desktop-native/Cargo.toml" | head -1)"
[ -n "$NATIVE_VERSION" ] || { echo "无法从 clients/desktop-native/Cargo.toml 读出 version" >&2; exit 1; }
[ -f "$NATIVE_BIN" ] || { echo "找不到 $NATIVE_BIN,先跑 cargo build --release --target $TARGET --manifest-path clients/desktop-native/Cargo.toml -p gateterm-desktop-native" >&2; exit 1; }

rm -rf "$NATIVE_APP" "$NATIVE_DMG"
mkdir -p "$NATIVE_APP/Contents/MacOS" "$NATIVE_APP/Contents/Resources"
cp "$NATIVE_BIN" "$NATIVE_APP/Contents/MacOS/gateterm-desktop-native"
chmod 755 "$NATIVE_APP/Contents/MacOS/gateterm-desktop-native"
cp "$ICON" "$NATIVE_APP/Contents/Resources/icon.icns"

cat > "$NATIVE_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Gateterm</string>
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
    <key>CFBundleVersion</key>
    <string>$NATIVE_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

sign_app "$NATIVE_APP"
build_dmg "$NATIVE_APP" "Gateterm" "$NATIVE_DMG" "native"
echo "$NATIVE_APP"
echo "$NATIVE_DMG"
