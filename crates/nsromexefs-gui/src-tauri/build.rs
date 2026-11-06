use std::path::PathBuf;

fn main() {
    ensure_bundled_cli();
    tauri_build::build()
}

/// Tauri validates `externalBin` while compiling the GUI crate, so a missing sidecar
/// would otherwise fail with a terse message. Point contributors to the setup command.
fn ensure_bundled_cli() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );
    let target = std::env::var("TARGET").expect("TARGET is set by Cargo");
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let sidecar = manifest_dir
        .join("binaries")
        .join(format!("nsromexefs-{target}{extension}"));
    if !sidecar.is_file() {
        panic!(
            "\n\nMissing bundled CLI sidecar: {}\nRun `npm run build:sidecar` from crates/nsromexefs-gui before building or testing the GUI.\n\n",
            sidecar.display()
        );
    }
    println!("cargo:rerun-if-changed={}", sidecar.display());
}
