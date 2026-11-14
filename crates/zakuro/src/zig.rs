//! Zig's C compiler, zig cc, which Zakuro setup puts in the tools folder
//! next to its program, so that recompiling a game needs nothing installed
//! by hand.

use std::path::{Path, PathBuf};

use recomp3ds::compile::Compiler;

/// the release that comes with Zakuro.
pub const VERSION: &str = "0.17.0";

/// the folders Zakuro looks in for Zig: the tools folder next to its
/// program, where a release puts it, then the one older versions downloaded
/// it into.
pub fn tools_dirs(data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_owned)) {
        dirs.push(exe_dir.join("tools"));
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|dir| !dir.is_empty()) {
            dirs.push(PathBuf::from(local).join("zakuro").join("tools"));
        }
    } else if let Some(dir) = data_dir {
        dirs.push(dir.join("tools"));
    }
    dirs
}

fn folder(tools: &Path) -> PathBuf {
    tools.join(format!("zig-{VERSION}"))
}

/// the compiler, when tools holds it.
pub fn installed(tools: &Path) -> Option<Compiler> {
    let program = folder(tools).join(if cfg!(windows) { "zig.exe" } else { "zig" });
    program.is_file().then(|| Compiler::new(program, &["cc"]))
}

/// the compiler that came with Zakuro, from the first folder holding it.
pub fn bundled(data_dir: Option<&Path>) -> Option<Compiler> {
    tools_dirs(data_dir).iter().find_map(|tools| installed(tools))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_zig_is_called_as_zig_cc() {
        let tools = std::env::temp_dir().join(format!("zakuro-zig-{}", std::process::id()));
        assert!(installed(&tools).is_none());
        let program = folder(&tools).join(if cfg!(windows) { "zig.exe" } else { "zig" });
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, b"").unwrap();
        assert_eq!(installed(&tools), Some(Compiler::new(&program, &["cc"])));
        std::fs::remove_dir_all(tools).unwrap();
    }

    #[test]
    fn the_folder_next_to_the_program_comes_first() {
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_owned();
        assert_eq!(tools_dirs(None).first(), Some(&exe_dir.join("tools")));
    }
}
