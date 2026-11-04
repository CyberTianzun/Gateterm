#!/usr/bin/env bash
# Runs the platform-independent unit tests (protocol, keyboard, chat, appearance and the Rust
# terminal core) on the Mac itself, without an iOS simulator runtime:
#     clients/ios/scripts/test-host.sh
# The same test files also run in Xcode against the app target (Product ▸ Test).
set -euo pipefail

IOS="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$IOS/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$IOS/Generated/gateterm_cloud_ffi.swift" ] || "$ROOT/scripts/build-ios-ffi.sh"
cargo build -q -p gateterm-cloud-ffi --lib --manifest-path "$ROOT/Cargo.toml"
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/Cargo.toml" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

OUT="$(mktemp -d "${TMPDIR:-/tmp}/gateterm-ios-tests.XXXXXX")"
trap 'rm -rf "$OUT"' EXIT
HEADERS="$(ls -d "$IOS"/Generated/GatetermFFI.xcframework/ios-arm64/Headers)"
PLATFORM="$(xcrun --show-sdk-platform-path --sdk macosx)"
XCTEST_FW="$PLATFORM/Developer/Library/Frameworks"
XCTEST_LIB="$PLATFORM/Developer/usr/lib"

# Everything except UIKit views, the app entry point and the UIKit-backed controller/cloud model.
SOURCES=()
while IFS= read -r file; do SOURCES+=("$file"); done < <(
  cd "$IOS/Gateterm" && find Wire Net Connection Agent Term Appearance I18n Push Controller -name '*.swift' \
    ! -name 'AppModel.swift' ! -name 'TermFonts.swift' | sed "s|^|$IOS/Gateterm/|")
SOURCES+=("$IOS/Generated/gateterm_cloud_ffi.swift")

swiftc -module-name Gateterm -emit-library -emit-module -enable-testing -parse-as-library -swift-version 5 \
  -Xcc -fmodule-map-file="$HEADERS/module.modulemap" -I "$HEADERS" \
  -L "$TARGET_DIR/debug" -lgateterm_cloud_ffi -Xlinker -rpath -Xlinker "$TARGET_DIR/debug" \
  -emit-module-path "$OUT/Gateterm.swiftmodule" -o "$OUT/libGateterm.dylib" "${SOURCES[@]}"

mkdir -p "$OUT/GatetermTests.xctest/Contents/MacOS"
swiftc -module-name GatetermTests -emit-library -parse-as-library -swift-version 5 \
  -I "$OUT" -Xcc -fmodule-map-file="$HEADERS/module.modulemap" -I "$HEADERS" \
  -F "$XCTEST_FW" -I "$XCTEST_LIB" -L "$XCTEST_LIB" -framework XCTest \
  -L "$OUT" -lGateterm -Xlinker -rpath -Xlinker "$OUT" -Xlinker -rpath -Xlinker "$XCTEST_FW" \
  -Xlinker -rpath -Xlinker "$XCTEST_LIB" -Xlinker -rpath -Xlinker "$TARGET_DIR/debug" \
  -o "$OUT/GatetermTests.xctest/Contents/MacOS/GatetermTests" "$IOS"/GatetermTests/*.swift
cat > "$OUT/GatetermTests.xctest/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>GatetermTests</string>
<key>CFBundleIdentifier</key><string>ai.eclosion.gateterm.host-tests</string>
<key>CFBundlePackageType</key><string>BNDL</string>
</dict></plist>
EOF
xcrun xctest "$OUT/GatetermTests.xctest"
