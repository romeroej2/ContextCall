use std::path::{Path, PathBuf};
use std::process::Command;

const FLUIDAUDIO_HELPER: &str = "contextcall-fluidaudio";

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        prepare_fluidaudio_helper();
    }
    tauri_build::build()
}

/// Builds the FluidAudio Swift sidecar and stages it where `tauri.macos.conf.json`'s
/// `externalBin` expects it (`binaries/<name>-<target-triple>`). Tauri copies it next
/// to the app executable for `tauri dev` and into Contents/MacOS for bundles.
///
/// Set CONTEXTCALL_SKIP_FLUIDAUDIO=1 to skip the Swift build (e.g. without Xcode 16);
/// a stub is staged instead and the engine reports itself unavailable.
fn prepare_fluidaudio_helper() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let package_dir = manifest_dir.join("helpers").join("fluidaudio-helper");
    let target = std::env::var("TARGET").unwrap();
    let staged = manifest_dir
        .join("binaries")
        .join(format!("{FLUIDAUDIO_HELPER}-{target}"));
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();

    println!(
        "cargo:rerun-if-changed={}",
        package_dir.join("Package.swift").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        package_dir.join("Package.resolved").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        package_dir.join("Sources").display()
    );
    println!("cargo:rerun-if-env-changed=CONTEXTCALL_SKIP_FLUIDAUDIO");

    // Parakeet on the Neural Engine is Apple Silicon only.
    let skip = std::env::var_os("CONTEXTCALL_SKIP_FLUIDAUDIO").is_some()
        || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64");
    if skip {
        write_stub(&staged);
        return;
    }

    let status = Command::new("swift")
        .args([
            "build",
            "-c",
            "release",
            "--arch",
            "arm64",
            "--package-path",
        ])
        .arg(&package_dir)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!(
            "FluidAudio helper build failed ({s}). It needs Xcode 16+ / Swift 6. \
             Set CONTEXTCALL_SKIP_FLUIDAUDIO=1 to build without the Neural Engine engine."
        ),
        Err(e) => panic!(
            "Could not run `swift` to build the FluidAudio helper: {e}. \
             Install Xcode command line tools, or set CONTEXTCALL_SKIP_FLUIDAUDIO=1."
        ),
    }
    let built = package_dir
        .join(".build")
        .join("arm64-apple-macosx")
        .join("release")
        .join(FLUIDAUDIO_HELPER);
    std::fs::copy(&built, &staged).unwrap_or_else(|e| {
        panic!(
            "Failed to stage {} -> {}: {e}",
            built.display(),
            staged.display()
        )
    });
}

/// Placeholder that answers every request with an error, so a build without the
/// Swift helper still bundles and the app surfaces a clear message.
fn write_stub(path: &Path) {
    let script = "#!/bin/sh\n\
        while read -r _; do\n\
        echo '{\"ok\":false,\"error\":\"This build of ContextCall does not include the FluidAudio helper.\"}'\n\
        done\n";
    std::fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
