fn main() {
    // rfd needs Common Controls v6 and its activation manifest for custom Windows buttons.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Octoterm.Desktop"))
            .expect("unable to embed Windows application manifest");
        compile_windows_icon();
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
}

fn compile_windows_icon() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
    );
    let ico_path = manifest_dir.join("assets").join("icon.ico");
    let rc_path = out_dir.join("icon.rc");
    let res_path = out_dir.join("icon.res");

    // Escape backslashes for Windows RC string literal
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
        assert!(status.success(), "rc.exe failed to compile icon.rc");
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
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
