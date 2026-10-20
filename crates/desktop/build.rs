fn main() {
    // rfd needs Common Controls v6 and its activation manifest for custom Windows buttons.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Octoterm.Desktop"))
            .expect("unable to embed Windows application manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
