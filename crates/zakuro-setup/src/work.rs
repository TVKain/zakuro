//! installing and uninstalling, away from the window, which shows how far
//! along they are.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::system;

/// the Zig release Zakuro recompiles with, which crates\zakuro\src\zig.rs
/// looks for in tools\zig-<version>, and its archive as
/// ziglang.org/download/index.json gives it.
pub const ZIG_VERSION: &str = "0.17.0";
const ZIG_URL: &str = "https://ziglang.org/download/0.17.0/zig-x86_64-windows-0.17.0.zip";
pub const ZIG_SIZE: u64 = 100_268_137;
const ZIG_SHA256: &str = "b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428";

/// the program installed, which the build put inside this one.
const ZAKURO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/zakuro.exe"));
const LICENSE: &str = include_str!("../../../LICENSE");
const FONT_LICENSE: &str = include_str!("../../zakuro/fonts/Inter-LICENSE.txt");

/// the files an install puts in its folder, besides Zig.
const INSTALLED: [&str; 4] = ["zakuro.exe", "uninstall.exe", "LICENSE.txt", "Inter-LICENSE.txt"];

/// the game files a library holds, as Zakuro lists them.
const GAME_FILES: [&str; 4] = ["3ds", "cci", "cxi", "cia"];

/// how far along the work is, which the window reads.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub step: String,
    /// how much of the step is done, when that is known.
    pub fraction: Option<f32>,
    pub finished: bool,
    /// why it stopped.
    pub error: Option<String>,
    /// what went wrong without stopping it.
    pub warnings: Vec<String>,
}

pub type Shared = Arc<Mutex<Progress>>;

fn set(progress: &Shared, step: &str, fraction: Option<f32>) {
    if let Ok(mut progress) = progress.lock() {
        progress.step = step.to_owned();
        progress.fraction = fraction;
    }
}

fn warn(progress: &Shared, warning: String) {
    if let Ok(mut progress) = progress.lock() {
        progress.warnings.push(warning);
    }
}

fn finish(progress: &Shared, result: Result<(), String>) {
    if let Ok(mut progress) = progress.lock() {
        progress.finished = true;
        progress.error = result.err();
    }
}

/// whether this setup carries Zakuro, which a build without it doesn't.
pub fn carries_zakuro() -> bool {
    !ZAKURO.is_empty()
}

/// the Zig in dir, when it is there.
pub fn zig_in(dir: &Path) -> PathBuf {
    dir.join("tools").join(format!("zig-{ZIG_VERSION}")).join("zig.exe")
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub dir: PathBuf,
    pub desktop: bool,
    pub zig: bool,
    pub version: String,
}

/// installs Zakuro as plan says, away from the window.
pub fn install(plan: InstallPlan, progress: Shared, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let result = install_now(&plan, &progress, &cancel);
        finish(&progress, result);
    });
}

fn install_now(plan: &InstallPlan, progress: &Shared, cancel: &AtomicBool) -> Result<(), String> {
    if !carries_zakuro() {
        return Err("This setup was built without Zakuro in it.".into());
    }
    if system::zakuro_running() {
        return Err("Zakuro is running. Close it, then try again.".into());
    }
    let dir = &plan.dir;
    set(progress, "Copying Zakuro", None);
    std::fs::create_dir_all(dir).map_err(|error| format!("The folder {} can't be made: {error}", dir.display()))?;
    replace(&dir.join("zakuro.exe"), ZAKURO)?;
    replace(&dir.join("LICENSE.txt"), LICENSE.as_bytes())?;
    replace(&dir.join("Inter-LICENSE.txt"), FONT_LICENSE.as_bytes())?;
    // this program stays as the uninstaller, unless it is that already
    let me = std::env::current_exe().map_err(|error| error.to_string())?;
    let uninstaller = dir.join("uninstall.exe");
    if !same_file(&me, &uninstaller) {
        let bytes = std::fs::read(&me).map_err(|error| format!("{}: {error}", me.display()))?;
        replace(&uninstaller, &bytes)?;
    }

    if plan.zig && !zig_in(dir).is_file() {
        if let Err(error) = get_zig(dir, progress, cancel) {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled. Zakuro is installed, without the compiler recompiling uses.".into());
            }
            warn(progress, format!("The compiler recompiling uses couldn't be set up: {error}. Run setup again to retry; games still play without it."));
        }
    }

    set(progress, "Adding shortcuts", None);
    if let Err(error) = system::make_shortcuts(dir, plan.desktop) {
        warn(progress, format!("The shortcuts couldn't be made: {error}"));
    }
    set(progress, "Adding Zakuro to Installed apps", None);
    if let Err(error) = system::register(dir, &plan.version, system::size_of(dir) / 1024) {
        warn(progress, error);
    }
    Ok(())
}

/// writes path whole, through a file beside it, so that a failure leaves
/// what was there.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fresh = path.with_extension("new");
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::write(&fresh, bytes).map_err(failed)?;
    std::fs::rename(&fresh, path).map_err(|error| {
        let _ = std::fs::remove_file(&fresh);
        failed(error)
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// downloads Zig, checks it is the release Zakuro wants, and unpacks it
/// into dir's tools folder.
fn get_zig(dir: &Path, progress: &Shared, cancel: &AtomicBool) -> Result<(), String> {
    let archive = std::env::temp_dir().join(format!("zakuro-zig-{ZIG_VERSION}.zip"));
    if !matches(&archive) {
        download(&archive, progress, cancel)?;
        set(progress, "Checking the compiler's download", None);
        if !matches(&archive) {
            let _ = std::fs::remove_file(&archive);
            return Err("the download isn't the release Zakuro expects".into());
        }
    }
    let result = unpack(&archive, &dir.join("tools").join(format!("zig-{ZIG_VERSION}")), progress, cancel);
    let _ = std::fs::remove_file(&archive);
    result
}

/// whether the file at path is Zig's archive, by size and SHA-256.
fn matches(path: &Path) -> bool {
    if std::fs::metadata(path).map(|meta| meta.len()).ok() != Some(ZIG_SIZE) {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return false,
        }
    }
    let hash: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    hash == ZIG_SHA256
}

/// downloads Zig to archive with the curl Windows comes with, reading how
/// far along it is from the file's size.
fn download(archive: &Path, progress: &Shared, cancel: &AtomicBool) -> Result<(), String> {
    let _ = std::fs::remove_file(archive);
    let mut child = system::hidden("curl")
        .args(["--location", "--fail", "--silent", "--show-error", "--retry", "3", "--output"])
        .arg(archive)
        .arg(ZIG_URL)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl couldn't start: {error}"))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(archive);
            return Err("cancelled".into());
        }
        let got = std::fs::metadata(archive).map(|meta| meta.len()).unwrap_or(0);
        let step = format!("Downloading the compiler recompiling uses, {} of {}", system::human(got), system::human(ZIG_SIZE));
        set(progress, &step, Some(got as f32 / ZIG_SIZE as f32));
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                let mut error = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                let error = error.trim().trim_start_matches("curl: ").to_owned();
                return Err(if error.is_empty() { "the download failed".into() } else { format!("the download failed, {error}") });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// unpacks Zig's archive into target, without the folder it is all in.
fn unpack(archive: &Path, target: &Path, progress: &Shared, cancel: &AtomicBool) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let staging = target.with_extension("unpacking");
    let _ = system::remove_all(&staging);
    let count = zip.len();
    for index in 0..count {
        if cancel.load(Ordering::Relaxed) {
            let _ = system::remove_all(&staging);
            return Err("cancelled".into());
        }
        if index % 64 == 0 {
            set(progress, "Unpacking the compiler", Some(index as f32 / count as f32));
        }
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        let inner: PathBuf = name.components().skip(1).collect();
        if inner.as_os_str().is_empty() {
            continue;
        }
        let path = staging.join(inner);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut out = std::fs::File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|error| format!("{}: {error}", path.display()))?;
        out.flush().map_err(|error| error.to_string())?;
    }
    if !staging.join("zig.exe").is_file() {
        let _ = system::remove_all(&staging);
        return Err("the archive has no zig.exe".into());
    }
    let _ = system::remove_all(target);
    std::fs::rename(&staging, target).map_err(|error| format!("{}: {error}", target.display()))
}

#[derive(Debug, Clone, Default)]
pub struct UninstallPlan {
    /// the folder Zakuro is installed in.
    pub dir: Option<PathBuf>,
    pub games: bool,
    pub saves: bool,
    pub settings: bool,
}

/// what the player's data takes, to show beside each choice.
#[derive(Debug, Clone, Default)]
pub struct DataSizes {
    pub library: Option<PathBuf>,
    pub games: u64,
    pub saves: u64,
    pub settings: u64,
}

pub fn data_sizes() -> DataSizes {
    let library = system::library_dir();
    let data = system::data_dir();
    let under = |names: &[&str]| data.as_ref().map_or(0, |data| names.iter().map(|name| system::size_of(&data.join(name))).sum());
    DataSizes {
        games: library.as_deref().map_or(0, system::size_of) + system::recompiled_dir().as_deref().map_or(0, system::size_of),
        saves: under(&SAVES),
        settings: under(&SETTINGS),
        library,
    }
}

/// what the data folder keeps of saves.
const SAVES: [&str; 2] = ["user", "states"];
/// and of settings and what the player added to games.
const SETTINGS: [&str; 6] = ["settings.toml", "cheats", "mods", "textures", "zakuro.log", "zakuro.previous.log"];

/// uninstalls Zakuro as plan says, away from the window.
pub fn uninstall(plan: UninstallPlan, progress: Shared) {
    std::thread::spawn(move || {
        let result = uninstall_now(&plan, &progress);
        finish(&progress, result);
    });
}

fn uninstall_now(plan: &UninstallPlan, progress: &Shared) -> Result<(), String> {
    if system::zakuro_running() {
        return Err("Zakuro is running. Close it, then try again.".into());
    }
    set(progress, "Removing Zakuro", None);
    system::remove_shortcuts();
    // only a folder setup installed to, which has the uninstaller, loses
    // its files
    if let Some(dir) = plan.dir.as_ref().filter(|dir| dir.join("uninstall.exe").is_file()) {
        for name in INSTALLED {
            let path = dir.join(name);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        system::remove_all(&dir.join("tools").join(format!("zig-{ZIG_VERSION}")))?;
        // the folder goes once nothing else is in it
        let _ = std::fs::remove_dir(dir.join("tools"));
        let _ = std::fs::remove_dir(dir);
    }
    system::unregister();
    // shader caches and older downloads, which Zakuro makes again
    if let Some(cache) = system::cache_dir() {
        if let Err(error) = system::remove_all(&cache) {
            warn(progress, error);
        }
    }

    let data = system::data_dir();
    if plan.games {
        set(progress, "Deleting your games", None);
        if let Some(library) = system::library_dir() {
            remove_library(&library, data.as_ref().is_some_and(|data| library == data.join("library")))?;
        }
        if let Some(recompiled) = system::recompiled_dir() {
            remove_recompiled(&recompiled)?;
        }
    }
    if let Some(data) = &data {
        if plan.saves {
            set(progress, "Deleting your saves", None);
            for name in SAVES {
                system::remove_all(&data.join(name))?;
            }
        }
        if plan.settings {
            set(progress, "Deleting your settings", None);
            for name in SETTINGS {
                system::remove_all(&data.join(name))?;
            }
        }
        let _ = std::fs::remove_dir(data);
    }
    Ok(())
}

/// deletes the games in library: all of it when it is Zakuro's own folder,
/// the game files alone and the folders they leave empty when the player
/// picked it, in case it holds more than games.
fn remove_library(library: &Path, own: bool) -> Result<(), String> {
    if own {
        return system::remove_all(library);
    }
    fn games_in(folder: &Path) -> Result<(), String> {
        let Ok(entries) = std::fs::read_dir(folder) else { return Ok(()) };
        for path in entries.flatten().map(|entry| entry.path()) {
            let game = path.extension().and_then(|ending| ending.to_str()).is_some_and(|ending| GAME_FILES.contains(&ending.to_ascii_lowercase().as_str()));
            if path.is_dir() {
                games_in(&path)?;
                let _ = std::fs::remove_dir(&path);
            } else if game {
                std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        Ok(())
    }
    games_in(library)?;
    let _ = std::fs::remove_dir(library);
    Ok(())
}

/// deletes the code recompiled for games, the files named for their ids.
fn remove_recompiled(folder: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Ok(()) };
    for path in entries.flatten().map(|entry| entry.path()) {
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
        if stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()) && path.is_file() {
            std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    let _ = std::fs::remove_dir(folder);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zakuro-setup-{name}-{}", std::process::id()));
        let _ = system::remove_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// a library the player picked keeps what isn't a game.
    #[test]
    fn a_picked_library_loses_only_its_games() {
        let dir = temp("library");
        for file in ["a.3ds", "Some game/b.cia", "notes.txt"] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        remove_library(&dir, false).unwrap();
        assert!(!dir.join("a.3ds").exists() && !dir.join("Some game").exists());
        assert!(dir.join("notes.txt").exists());
        remove_library(&dir, true).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn recompiled_code_goes_by_its_file_names() {
        let dir = temp("recompiled");
        for file in ["00040000001B5000.dll", "00040000001B5000.addons", "readme.txt"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        remove_recompiled(&dir).unwrap();
        assert!(!dir.join("00040000001B5000.dll").exists() && !dir.join("00040000001B5000.addons").exists());
        assert!(dir.join("readme.txt").exists());
        let _ = system::remove_all(&dir);
    }
}
