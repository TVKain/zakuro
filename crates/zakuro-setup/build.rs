//! puts zakuro.exe inside the setup program, which installs it, and gives
//! the setup program Zakuro's icon and a manifest that keeps Windows from
//! running it as administrator, as it would a program named setup.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // the exe a release build made, or the one named by ZAKURO_EXE. without
    // either, as in a check of the workspace, the setup carries nothing
    println!("cargo:rerun-if-env-changed=ZAKURO_EXE");
    let payload = std::env::var_os("ZAKURO_EXE").map(PathBuf::from).unwrap_or_else(|| manifest_dir.join("../../target/release/zakuro.exe"));
    println!("cargo:rerun-if-changed={}", payload.display());
    let embedded = out_dir.join("zakuro.exe");
    if payload.is_file() {
        std::fs::copy(&payload, &embedded).expect("copy zakuro.exe into the setup");
    } else {
        println!("cargo:warning=no zakuro.exe at {}, the setup carries none", payload.display());
        std::fs::write(&embedded, b"").unwrap();
    }

    let windows = std::env::var("CARGO_CFG_TARGET_FAMILY").is_ok_and(|family| family == "windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if windows && msvc {
        let manifest = manifest_dir.join("setup.manifest");
        let icon = manifest_dir.join("../../packaging/zakuro.res");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rerun-if-changed={}", icon.display());
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", canonical(&manifest).display());
        if icon.is_file() {
            println!("cargo:rustc-link-arg-bins={}", canonical(&icon).display());
        }
    }
}

fn canonical(path: &Path) -> PathBuf {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    // link.exe doesn't take the \?\ prefix canonicalize adds
    PathBuf::from(path.to_string_lossy().trim_start_matches(r"\?\"))
}
