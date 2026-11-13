//! the main thread runs the game's code, whose stack is 8 MiB on Linux
//! and only 1 MiB on Windows unless the program asks for more.

fn main() {
    let windows = std::env::var("CARGO_CFG_TARGET_FAMILY").is_ok_and(|family| family == "windows");
    if windows {
        let stack = 8 << 20;
        match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{stack}"),
            _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{stack}"),
        }

        // Link the Windows application icon resource
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let rc_path = std::path::Path::new(&manifest_dir).join("../../packaging/zakuro.rc");
        let ico_path = std::path::Path::new(&manifest_dir).join("../../packaging/zakuro.ico");
        let res_path = std::path::Path::new(&manifest_dir).join("../../packaging/zakuro.res");

        println!("cargo:rerun-if-changed={}", rc_path.display());
        println!("cargo:rerun-if-changed={}", ico_path.display());

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let target_res = std::path::Path::new(&out_dir).join("zakuro.res");

        let mut compiled = false;
        // Try rc.exe from PATH or Windows SDK
        let rc_candidates = [
            "rc.exe".to_string(),
            r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe".to_string(),
            r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe".to_string(),
        ];
        for candidate in &rc_candidates {
            if let Ok(status) = std::process::Command::new(candidate)
                .arg("/fo")
                .arg(&target_res)
                .arg(&rc_path)
                .status()
            {
                if status.success() {
                    compiled = true;
                    break;
                }
            }
        }

        // If rc.exe was not found or failed, try windres
        if !compiled {
            if let Ok(status) = std::process::Command::new("windres.exe")
                .arg("-i")
                .arg(&rc_path)
                .arg("-O")
                .arg("coff")
                .arg("-o")
                .arg(&target_res)
                .status()
            {
                if status.success() {
                    compiled = true;
                }
            }
        }

        let res_to_link = if compiled {
            target_res
        } else if res_path.exists() {
            res_path
        } else {
            return;
        };

        if let Ok(canon) = res_to_link.canonicalize() {
            println!("cargo:rustc-link-arg-bins={}", canon.display());
        }
    }
}
