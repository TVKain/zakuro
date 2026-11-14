//! what installing touches on Windows: where things go, the entry in
//! Installed apps, the shortcuts, and whether Zakuro is running.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// starts a console program with no window of its own.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// the key Installed apps lists Zakuro under, for this user.
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Zakuro";

/// a console program, run without a window.
pub fn hidden(program: &str) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// where Zakuro installs for one user unless told otherwise, as other
/// per-user programs do.
pub fn default_install_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").unwrap_or_else(std::env::temp_dir).join("Programs").join("Zakuro")
}

/// Zakuro's data folder: its games, saves and settings.
pub fn data_dir() -> Option<PathBuf> {
    Some(env_dir("APPDATA")?.join("zakuro"))
}

/// where the recompiled code of games is kept.
pub fn recompiled_dir() -> Option<PathBuf> {
    Some(env_dir("APPDATA")?.join("3dsrecomp"))
}

/// Zakuro's caches, and the Zig older versions downloaded.
pub fn cache_dir() -> Option<PathBuf> {
    Some(env_dir("LOCALAPPDATA")?.join("zakuro"))
}

/// the folder games are kept in: the one settings.toml names, or the data
/// folder's library.
pub fn library_dir() -> Option<PathBuf> {
    let data = data_dir()?;
    let settings = std::fs::read_to_string(data.join("settings.toml")).unwrap_or_default();
    // a line library = '...' or "..." at the top of the file, before any table
    let named = settings
        .lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .find_map(|line| {
            let value = line.trim().strip_prefix("library")?.trim_start().strip_prefix('=')?.trim();
            let unquoted = match value.chars().next()? {
                '\'' => value.strip_prefix('\'')?.strip_suffix('\'')?.to_owned(),
                '"' => value.strip_prefix('"')?.strip_suffix('"')?.replace("\\\\", "\\"),
                _ => return None,
            };
            Some(PathBuf::from(unquoted))
        });
    Some(named.unwrap_or_else(|| data.join("library")))
}

/// where Zakuro is installed now, as Installed apps has it.
pub fn installed_at() -> Option<PathBuf> {
    let output = hidden("reg").args(["query", UNINSTALL_KEY, "/v", "InstallLocation"]).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.trim_start().starts_with("InstallLocation"))?;
    let value = line.split("REG_SZ").nth(1)?.trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn reg_set(name: &str, kind: &str, value: &str) -> Result<(), String> {
    let status = hidden("reg").args(["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", value, "/f"]).status().map_err(|error| error.to_string())?;
    status.success().then_some(()).ok_or_else(|| format!("Windows didn't take the {name} of the Installed apps entry"))
}

/// lists Zakuro in Installed apps, with the uninstaller in dir.
pub fn register(dir: &Path, version: &str, size_kb: u64) -> Result<(), String> {
    let exe = dir.join("zakuro.exe");
    let uninstaller = dir.join("uninstall.exe");
    reg_set("DisplayName", "REG_SZ", "Zakuro")?;
    reg_set("DisplayVersion", "REG_SZ", version)?;
    reg_set("Publisher", "REG_SZ", "Zakuro")?;
    reg_set("DisplayIcon", "REG_SZ", &exe.display().to_string())?;
    reg_set("InstallLocation", "REG_SZ", &dir.display().to_string())?;
    reg_set("UninstallString", "REG_SZ", &format!("\"{}\"", uninstaller.display()))?;
    reg_set("QuietUninstallString", "REG_SZ", &format!("\"{}\" --quiet", uninstaller.display()))?;
    reg_set("EstimatedSize", "REG_DWORD", &size_kb.min(u32::MAX as u64).to_string())?;
    reg_set("NoModify", "REG_DWORD", "1")?;
    reg_set("NoRepair", "REG_DWORD", "1")?;
    Ok(())
}

/// takes Zakuro out of Installed apps.
pub fn unregister() {
    let _ = hidden("reg").args(["delete", UNINSTALL_KEY, "/f"]).status();
}

/// runs a PowerShell script, with what it needs passed in the environment
/// rather than quoted into it.
fn powershell(script: &str, env: &[(&str, &Path)]) -> Result<(), String> {
    let mut command = hidden("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    match output.status.success() {
        true => Ok(()),
        // PowerShell's first line says what failed, the rest where
        false => Err(String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("PowerShell failed").trim().to_owned()),
    }
}

const SHORTCUT: &str = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:LINK); \
    $s.TargetPath = $env:TARGET; $s.WorkingDirectory = $env:WORKDIR; $s.Description = 'Nintendo 3DS emulator'; $s.Save()";

/// the Start menu's folder of programs, for this user.
fn start_menu() -> Option<PathBuf> {
    Some(env_dir("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs"))
}

/// a shortcut to Zakuro in dir, in the Start menu and, when asked, on the
/// desktop wherever Windows keeps it.
pub fn make_shortcuts(dir: &Path, desktop: bool) -> Result<(), String> {
    let exe = dir.join("zakuro.exe");
    if let Some(menu) = start_menu() {
        let _ = std::fs::create_dir_all(&menu);
        powershell(SHORTCUT, &[("LINK", &menu.join("Zakuro.lnk")), ("TARGET", &exe), ("WORKDIR", dir)])?;
    }
    if desktop {
        let script = format!("$env:LINK = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Zakuro.lnk'; {SHORTCUT}");
        powershell(&script, &[("TARGET", &exe), ("WORKDIR", dir)])?;
    }
    Ok(())
}

/// takes away the shortcuts setup made.
pub fn remove_shortcuts() {
    if let Some(menu) = start_menu() {
        let _ = std::fs::remove_file(menu.join("Zakuro.lnk"));
    }
    let _ = powershell("Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Zakuro.lnk') -ErrorAction SilentlyContinue", &[]);
}

/// whether a Zakuro is running, which holds its program file open.
pub fn zakuro_running() -> bool {
    hidden("tasklist")
        .args(["/FI", "IMAGENAME eq zakuro.exe", "/NH"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).to_ascii_lowercase().contains("zakuro.exe"))
}

/// deletes path, this program, once it has ended however it ends: a
/// hidden PowerShell waits for it, then deletes it.
pub fn delete_after_exit(path: &Path) {
    let script = "Wait-Process -Id $env:SETUP_PID -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath $env:SETUP_EXE -Force -ErrorAction SilentlyContinue";
    let _ = hidden("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("SETUP_PID", std::process::id().to_string())
        .env("SETUP_EXE", path)
        .spawn();
}

/// the bytes the files under path take.
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    // a texture pack linked in is counted where it is kept
    if meta.file_type().is_symlink() || is_junction(path) {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map(|entries| entries.flatten().map(|entry| size_of(&entry.path())).sum()).unwrap_or(0)
}

/// whether path is a junction, which Zakuro links texture packs with.
fn is_junction(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const REPARSE_POINT: u32 = 0x400;
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_attributes() & REPARSE_POINT != 0)
}

/// deletes path and everything in it, a link inside going without what it
/// points to. none there is fine.
pub fn remove_all(path: &Path) -> Result<(), String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return Ok(()) };
    let result = if meta.file_type().is_symlink() || is_junction(path) {
        // a junction or folder link is removed as a folder, its target kept
        std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path))
    } else if meta.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?.flatten() {
            remove_all(&entry.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|error| format!("{}: {error}", path.display()))
}

/// a size as people read it.
pub fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(512), "512 bytes");
        assert_eq!(human(3 << 20), "3 MB");
        assert_eq!(human(5 << 30), "5.0 GB");
    }
}
