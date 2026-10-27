#!/usr/bin/env bash
# 构建 Android 用的 Rust 库(arm64-v8a)与 UniFFI Kotlin 绑定。
# 由 clients/android 的 Gradle 任务 buildCloudFfi 调用,也可手动运行。
# 用法:scripts/build-android-ffi.sh <jniLibs 输出目录> <Kotlin 输出目录>
#
# 安全提示:<Kotlin 输出目录> 会被 `rm -rf` 清空再重新生成,脚本只在该路径
# 含 `/build/` 或 `/generated/` 这类一看就是构建产物的路径片段、或落在
# `$TMPDIR`/`/tmp` 下时才会真的删——传别的路径(比如不小心传成项目根目录)会直接拒绝退出。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Gradle 在 Windows 上传入反斜杠路径。统一成正斜杠后,下面的安全检查和 [ -f ]
# 在 Git Bash 里才能命中,Windows 版 cargo 也接受这种路径。
normalize_path() {
  local p="$1"
  p="${p//\\//}"
  printf '%s' "$p"
}
OUT_JNI="$(normalize_path "${1:?需要 jniLibs 输出目录}")"
OUT_KT="$(normalize_path "${2:?需要 Kotlin 输出目录}")"
: "${ANDROID_NDK_HOME:?需要 ANDROID_NDK_HOME(本机:/Users/hiro/frameworks/android-ndk-r30)}"
export ANDROID_NDK_HOME="$(normalize_path "$ANDROID_NDK_HOME")"

cd "$ROOT"

# 给 Android 目标单独指定 rustflags:target 级的设置优先于 `build.rustflags`,
# 用户全局 cargo 配置里若有 `-Clinker=rust-lld` 之类的设置,不会盖掉 cargo-ndk 的 NDK 链接器。
# 顺带按 16KB 页对齐(新版 Android 对原生库的要求)——无论调用方有没有预先设置这个变量,
# 都在后面追加这条 flag,而不是整个替换掉调用方的设置。
PAGE_SIZE_FLAG="-Clink-arg=-Wl,-z,max-page-size=16384"
if [ -n "${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-}" ]; then
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS} ${PAGE_SIZE_FLAG}"
else
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${PAGE_SIZE_FLAG}"
fi
# 全局 RUSTFLAGS 的优先级比 target 级的 CARGO_TARGET_*_RUSTFLAGS 更高,会整个盖掉上面的设置
# (包括 16KB 页对齐那条 flag),这里只是提醒,不强行 unset——调用方可能是故意设的。
if [ -n "${RUSTFLAGS:-}" ]; then
  echo "警告:检测到全局 RUSTFLAGS,会覆盖本脚本给 Android 目标设置的 rustflags(含 16KB 页对齐)" >&2
fi
cargo ndk -t arm64-v8a -o "$OUT_JNI" build --release -p gateterm-cloud-ffi

# 绑定从本机 debug 构建生成:release 的 `strip` 会剥掉 .symtab,UniFFI 读不到元数据、
# 静默生成空目录;接口元数据与目标平台无关,host 库生成的 Kotlin 与 arm64 .so 完全对应。
cargo build -q -p gateterm-cloud-ffi
if command -v python3 >/dev/null 2>&1; then
  PYTHON=python3
elif command -v python >/dev/null 2>&1; then
  PYTHON=python
else
  echo "需要 python3(或 python)来解析 cargo metadata" >&2
  exit 1
fi
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | "$PYTHON" -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
TARGET_DIR="$(normalize_path "$TARGET_DIR")"
HOST_LIB=""
for f in \
  "$TARGET_DIR/debug/libgateterm_cloud_ffi.dylib" \
  "$TARGET_DIR/debug/libgateterm_cloud_ffi.so" \
  "$TARGET_DIR/debug/libgateterm_cloud_ffi.dll" \
  "$TARGET_DIR/debug/gateterm_cloud_ffi.dll"
do
  [ -f "$f" ] && { HOST_LIB="$f"; break; }
done
[ -n "$HOST_LIB" ] || { echo "找不到本机 debug 版 libgateterm_cloud_ffi" >&2; exit 1; }

# 只在看起来安全的路径下清空重建,避免传错路径时把不相关的目录整个删掉。
case "$OUT_KT" in
  */build/*|*/generated/*|"${TMPDIR:-/tmp}"*|/tmp/*) rm -rf "$OUT_KT" ;;
  *)
    echo "拒绝清空 \$OUT_KT=$OUT_KT:路径不含 /build/ 或 /generated/ 片段,也不在 \$TMPDIR 或 /tmp 下" >&2
    exit 1
    ;;
esac
cargo run -q -p gateterm-cloud-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language kotlin --out-dir "$OUT_KT" --no-format
# 空输出说明元数据没读到:必须失败,别让 Gradle 拿着空目录继续
[ -n "$(find "$OUT_KT" -name '*.kt' -print -quit)" ] || { echo "UniFFI 没有生成任何 Kotlin 文件" >&2; exit 1; }
