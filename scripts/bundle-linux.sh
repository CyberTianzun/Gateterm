#!/usr/bin/env bash
# 把 gateterm-desktop-web 与 gateterm-desktop-native 打成 Linux 的 tar.gz 安装包。
# 包内:可执行文件 + hicolor 图标 + 启动器 + install.sh / uninstall.sh(装到 ~/.local,不需要 root)。
#
# 托盘走 freedesktop StatusNotifierItem(D-Bus),需要状态栏支持 SNI:
# Omarchy / Hyprland + Waybar、KDE Plasma、GNOME + AppIndicator 扩展等。
set -euo pipefail

TARGET="${1:-x86_64-unknown-linux-gnu}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICON_DIR="$ROOT/clients/desktop-web/assets/linux"
BUNDLE_DIR="$ROOT/target/bundle"

STAGE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/gateterm-linux.XXXXXX")"
trap 'rm -rf "$STAGE_ROOT"' EXIT

# $1 包名(= 可执行文件名) $2 可执行文件路径 $3 启动器里显示的名字 $4 一句话说明
package() {
  local name="$1" bin="$2" display="$3" comment="$4"
  [ -f "$bin" ] || { echo "找不到 $bin,先 cargo build --release --target $TARGET" >&2; exit 1; }

  local stage="$STAGE_ROOT/$name"
  mkdir -p "$stage"
  install -m 755 "$bin" "$stage/$name"
  for size in 16 24 32 48 64 128 256 512; do
    install -D -m 644 "$ICON_DIR/icon-$size.png" "$stage/icons/hicolor/${size}x${size}/apps/gateterm.png"
  done

  # 启动器模板:Exec 留占位符,由 install.sh 换成装好后的绝对路径 —— 桌面环境起进程时
  # 的 PATH 里不一定有 ~/.local/bin。
  cat > "$stage/$name.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$display
Comment=$comment
Exec="@EXEC@"
Icon=gateterm
Terminal=false
Categories=Development;System;
StartupNotify=false
DESKTOP

  cat > "$stage/install.sh" <<'INSTALL'
#!/usr/bin/env bash
# 安装到当前用户:可执行文件 -> ~/.local/bin,图标与启动器 -> ~/.local/share。
# 想装到别处:PREFIX=/opt/gateterm ./install.sh(需要对应目录的写权限)。
set -euo pipefail
NAME="@NAME@"
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
[ "$PREFIX" = "$HOME/.local" ] || DATA="$PREFIX/share"
BIN="$PREFIX/bin/$NAME"

mkdir -p "$PREFIX/bin" "$DATA/applications"
# 先写临时文件再 rename:正在运行的旧版本不受影响(它持有的是旧 inode)
install -m 755 "$SRC/$NAME" "$BIN.new"
mv -f "$BIN.new" "$BIN"
cp -r "$SRC/icons" "$DATA/"
sed "s|@EXEC@|$BIN|" "$SRC/$NAME.desktop" > "$DATA/applications/$NAME.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true

# 已经在跑的旧版本:停掉并用新版本重新拉起。按完整命令行匹配:进程名超过 15 个字符,
# pgrep -x 只看被截断的 comm,永远匹配不上。
running() { pgrep -u "$(id -u)" -f "(^|/)$NAME( |\$)"; }
if running >/dev/null 2>&1; then
  pkill -u "$(id -u)" -f "(^|/)$NAME( |\$)" || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do running >/dev/null 2>&1 || break; sleep 0.5; done
  if [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
    nohup "$BIN" >/dev/null 2>&1 &
    echo "已重启 $NAME"
  fi
fi
echo "已安装 $BIN"
echo "从应用启动器里打开「$(sed -n 's/^Name=//p' "$SRC/$NAME.desktop")」,或直接运行 $BIN"
INSTALL
  sed -i "s|@NAME@|$name|" "$stage/install.sh"
  chmod 755 "$stage/install.sh"

  cat > "$stage/uninstall.sh" <<'UNINSTALL'
#!/usr/bin/env bash
# 卸载程序本身;配置与日志保留。
set -euo pipefail
NAME="@NAME@"
PREFIX="${PREFIX:-$HOME/.local}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
[ "$PREFIX" = "$HOME/.local" ] || DATA="$PREFIX/share"
pkill -u "$(id -u)" -f "(^|/)$NAME( |\$)" 2>/dev/null || true
rm -f "$PREFIX/bin/$NAME" "$DATA/applications/$NAME.desktop" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$NAME.desktop"
echo "已卸载 $NAME"
UNINSTALL
  sed -i "s|@NAME@|$name|" "$stage/uninstall.sh"
  chmod 755 "$stage/uninstall.sh"

  local out="$BUNDLE_DIR/$name-linux.tar.gz"
  rm -f "$out"
  tar -C "$STAGE_ROOT" -czf "$out" "$name"
  echo "$out"
}

mkdir -p "$BUNDLE_DIR"

if [ "${SKIP_WEB:-0}" != "1" ]; then
  package gateterm-desktop-web \
    "$ROOT/target/$TARGET/release/gateterm-desktop-web" \
    "Gateterm Desktop Web" \
    "gateterm 托盘服务:在浏览器里使用本机终端"
fi

if [ "${SKIP_NATIVE:-0}" != "1" ]; then
  package gateterm-desktop-native \
    "$ROOT/clients/desktop-native/target/$TARGET/release/gateterm-desktop-native" \
    "Gateterm" \
    "gateterm GPU 终端工作台"
fi
