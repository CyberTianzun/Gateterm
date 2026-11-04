#!/usr/bin/env bash
# 构建 iOS 用的 Rust 静态库(真机 arm64 + 模拟器 arm64)、XCFramework 与 UniFFI Swift 绑定。
# 由 clients/ios 的 Xcode 预构建脚本调用,也可手动运行。
# 用法:scripts/build-ios-ffi.sh [--if-stale] [输出目录,默认 clients/ios/Generated]
#   --if-stale  Rust 源码与 Cargo 清单都没比上次输出新时直接跳过(Xcode 每次构建都会调用)
#
# 输出目录整个是生成物(已在 .gitignore):只在路径含 /Generated 或 /build/ 时才会清空重建。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Xcode 的脚本环境导出几百个变量(SDKROOT、PLATFORM_NAME、*_DEPLOYMENT_TARGET…),会把 cc-rs 编
# ring 等 C 依赖时的 SDK 选择带偏(报 TargetConditionals.h 找不到)。在干净环境里重新执行自己。
if [ -n "${XCODE_VERSION_ACTUAL:-}" ] && [ -z "${GATETERM_FFI_CLEAN_ENV:-}" ]; then
  exec env -i HOME="$HOME" USER="${USER:-}" TMPDIR="${TMPDIR:-/tmp}" GATETERM_FFI_CLEAN_ENV=1 \
    PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    ${CARGO_HOME:+CARGO_HOME="$CARGO_HOME"} ${RUSTUP_HOME:+RUSTUP_HOME="$RUSTUP_HOME"} \
    bash "$0" "$@"
fi
IF_STALE=0
if [ "${1:-}" = "--if-stale" ]; then IF_STALE=1; shift; fi
OUT="${1:-$ROOT/clients/ios/Generated}"
STAMP="$OUT/.stamp"
if [ "$IF_STALE" = 1 ] && [ -f "$STAMP" ] && [ -d "$OUT/GatetermFFI.xcframework" ]; then
  newer="$(find "$ROOT/crates/client-core" "$ROOT/crates/cloud-client" "$ROOT/crates/cloud-ffi" \
    "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$0" -newer "$STAMP" -type f -print -quit)"
  [ -z "$newer" ] && { echo "iOS FFI 已是最新"; exit 0; }
fi
case "$OUT" in
  */Generated|*/Generated/|*/build/*) ;;
  *) echo "拒绝写入 $OUT:路径须以 /Generated 结尾或位于 /build/ 下" >&2; exit 1 ;;
esac

cd "$ROOT"
# Xcode 的脚本环境里 PATH 很短,cargo 通常装在 ~/.cargo/bin。
export PATH="$HOME/.cargo/bin:$PATH"
# Xcode 会导出 SDKROOT 等变量指向 iOS SDK,host 上的 build script / bindgen 会被它带偏。
unset SDKROOT IPHONEOS_DEPLOYMENT_TARGET MACOSX_DEPLOYMENT_TARGET 2>/dev/null || true
export IPHONEOS_DEPLOYMENT_TARGET=17.0

PROFILE="${GATETERM_FFI_PROFILE:-release}"
PROFILE_FLAG=$([ "$PROFILE" = release ] && echo --release || echo "")
for target in aarch64-apple-ios aarch64-apple-ios-sim; do
  rustup target list --installed | grep -qx "$target" || {
    echo "缺少 Rust 目标 $target,先运行:rustup target add $target" >&2; exit 1; }
  cargo build $PROFILE_FLAG -p gateterm-cloud-ffi --lib --target "$target"
done

TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

# 绑定从 host debug 库生成:release 的 strip 会剥掉 UniFFI 元数据。接口与目标平台无关。
cargo build -q -p gateterm-cloud-ffi --lib
HOST_LIB="$TARGET_DIR/debug/libgateterm_cloud_ffi.dylib"
[ -f "$HOST_LIB" ] || { echo "找不到 $HOST_LIB" >&2; exit 1; }

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/gateterm-ios-ffi.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
cargo run -q -p gateterm-cloud-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language swift --out-dir "$STAGE/swift" --no-format
[ -f "$STAGE/swift/gateterm_cloud_ffi.swift" ] || { echo "UniFFI 没有生成 Swift 文件" >&2; exit 1; }

mkdir -p "$STAGE/headers"
cp "$STAGE/swift/gateterm_cloud_ffiFFI.h" "$STAGE/headers/"
# XCFramework 的 Headers 里需要标准文件名 module.modulemap,Swift 才能 `import gateterm_cloud_ffiFFI`。
cp "$STAGE/swift/gateterm_cloud_ffiFFI.modulemap" "$STAGE/headers/module.modulemap"

rm -rf "$OUT"
mkdir -p "$OUT"
xcodebuild -create-xcframework \
  -library "$TARGET_DIR/aarch64-apple-ios/$PROFILE/libgateterm_cloud_ffi.a" -headers "$STAGE/headers" \
  -library "$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE/libgateterm_cloud_ffi.a" -headers "$STAGE/headers" \
  -output "$OUT/GatetermFFI.xcframework" >/dev/null
cp "$STAGE/swift/gateterm_cloud_ffi.swift" "$OUT/"
touch "$STAMP"
echo "iOS FFI 已生成到 $OUT"
