fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // Increase Windows main-thread stack size from the 1 MB MSVC default to 8 MB
        // (matching macOS/Linux defaults and Zed/GPUI requirements for unoptimized debug builds).
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
        compile_windows_icon();
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../crates/desktop/assets/icon.ico");
}

fn compile_windows_icon() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
    );
    let ico_path = manifest_dir
        .join("../../crates/desktop/assets/icon.ico")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../crates/desktop/assets/icon.ico"));
    if !ico_path.is_file() {
        return;
    }
    let rc_path = out_dir.join("icon.rc");
    let res_path = out_dir.join("icon.res");

    let ico_escaped = ico_path.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(&rc_path, format!("1 ICON \"{ico_escaped}\"\r\n"))
        .expect("unable to write icon.rc");

    if let Some(rc_exe) = find_rc_exe() {
        let status = std::process::Command::new(rc_exe)
            .args(["/nologo", "/fo"])
            .arg(&res_path)
            .arg(&rc_path)
            .status()
            .expect("failed to execute rc.exe");
        if status.success() {
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
    }
}

fn find_rc_exe() -> Option<std::path::PathBuf> {
    if std::process::Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(std::path::PathBuf::from("rc.exe"));
    }
    let kits_root = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(kits_root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("x64").join("rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}
