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
cargo ndk -t arm64-v8a -o "$OUT_JNI" build --release -p octoterm-cloud-ffi
rm -rf "$OUT_KT"
cargo run -q -p octoterm-cloud-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$OUT_JNI/arm64-v8a/liboctoterm_cloud_ffi.so" \
  --language kotlin --out-dir "$OUT_KT" --no-format
