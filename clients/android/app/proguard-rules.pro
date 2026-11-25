# Gateterm Android R8 / ProGuard rules.
# Keep only reflection, JNI/JNA, and runtime-contract entry points so R8 can
# shrink and obfuscate the rest of the application and third-party DEX code.

# Preserve stack-trace line numbers for Play Console deobfuscation without
# leaking original source file names.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile

# ---------------------------------------------------------------------------
# JNA & UniFFI (crates/cloud-ffi -> uniffi.gateterm_cloud_ffi)
# ---------------------------------------------------------------------------
# JNA ships no consumer ProGuard rules. UniFFI loads native C symbols through
# dynamic proxies (com.sun.jna.Library), passes structs by reflecting on
# @Structure.FieldOrder and @JvmField names (Structure, ByValue, ByReference),
# and invokes JVM callbacks from Rust (com.sun.jna.Callback).
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.Library { *; }
-keep class * implements com.sun.jna.Callback { *; }
-keep class * extends com.sun.jna.Structure {
    public <init>(...);
    <fields>;
}
-keep class * extends com.sun.jna.Structure$ByValue { *; }
-keep class * extends com.sun.jna.Structure$ByReference { *; }
-keep class * extends com.sun.jna.ptr.ByReference { *; }
-keep class * extends com.sun.jna.PointerType { *; }
-keep @com.sun.jna.Structure$FieldOrder class * {
    public <init>(...);
    <fields>;
}
-dontwarn com.sun.jna.**
