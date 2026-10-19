#!/usr/bin/env bash
# 构建 Android 用的 Rust 库(arm64-v8a)与 UniFFI Kotlin 绑定。
# 由 clients/android 的 Gradle 任务 buildCloudFfi 调用,也可手动运行。
# 用法:scripts/build-android-ffi.sh <jniLibs 输出目录> <Kotlin 输出目录>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_JNI="${1:?需要 jniLibs 输出目录}"
OUT_KT="${2:?需要 Kotlin 输出目录}"
: "${ANDROID_NDK_HOME:?需要 ANDROID_NDK_HOME(本机:/Users/hiro/frameworks/android-ndk-r30)}"

cd "$ROOT"

# 给 Android 目标单独指定 rustflags:target 级的设置优先于 `build.rustflags`,
# 用户全局 cargo 配置里若有 `-Clinker=rust-lld` 之类的设置,不会盖掉 cargo-ndk 的 NDK 链接器。
# 顺带按 16KB 页对齐(新版 Android 对原生库的要求)。
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:--Clink-arg=-Wl,-z,max-page-size=16384}"
cargo ndk -t arm64-v8a -o "$OUT_JNI" build --release -p octoterm-cloud-ffi

# 绑定从本机 debug 构建生成:release 的 `strip` 会剥掉 .symtab,UniFFI 读不到元数据、
# 静默生成空目录;接口元数据与目标平台无关,host 库生成的 Kotlin 与 arm64 .so 完全对应。
cargo build -q -p octoterm-cloud-ffi
HOST_LIB=""
for f in target/debug/liboctoterm_cloud_ffi.dylib target/debug/liboctoterm_cloud_ffi.so; do
  [ -f "$f" ] && { HOST_LIB="$f"; break; }
done
[ -n "$HOST_LIB" ] || { echo "找不到本机 debug 版 liboctoterm_cloud_ffi" >&2; exit 1; }
rm -rf "$OUT_KT"
cargo run -q -p octoterm-cloud-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language kotlin --out-dir "$OUT_KT" --no-format
# 空输出说明元数据没读到:必须失败,别让 Gradle 拿着空目录继续
find "$OUT_KT" -name '*.kt' | grep -q . || { echo "UniFFI 没有生成任何 Kotlin 文件" >&2; exit 1; }
