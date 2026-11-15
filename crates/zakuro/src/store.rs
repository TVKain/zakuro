//! what Zakuro keeps for the player: the games added to its library, and
//! each game's texture packs and cheats. a file or folder dropped on the
//! window, or picked, is looked at to tell what it is, then put where it
//! goes away from the interface, with its progress to show.
//!
//! the library is a folder of Zakuro's own, its data folder's library
//! folder. texture packs
//! copied in go to its data folder's textures folder once, and each game
//! they are for gets a link to them in its mods folder, so a pack made for
//! a pair of games, Sun and Moon say, is kept once. a pack used from where
//! it is gets only the links.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zakuro_fs::TitleKind;

use crate::library::{self, Source};

/// the folder the library keeps its games in, the data folder's library.
pub fn library_dir(data_dir: Option<&Path>) -> Option<PathBuf> {
    Some(data_dir?.join("library"))
}

/// the folders the library lists games from: Zakuro's own, with a folder
/// for each game or none.
pub fn sources(data_dir: Option<&Path>) -> Vec<Source> {
    library_dir(data_dir).map(|folder| Source { folder, depth: 2 }).into_iter().collect()
}

/// the folder texture packs copied into Zakuro are kept in, each once.
pub fn shared_textures(data_dir: &Path) -> PathBuf {
    data_dir.join("textures")
}

/// what a file or folder handed to Zakuro turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// a game, or an update, DLC or Download Play child of one.
    Title { path: PathBuf, title_id: u64, kind: TitleKind, version: u16, name: String, size: u64 },
    /// games in a zip, unzipped into the library.
    ZippedGames { path: PathBuf, files: usize, size: u64 },
    /// a texture pack, a folder of pictures or a zip of them.
    Textures { path: PathBuf, name: String, zip: bool, images: usize, size: u64, title_id: Option<u64> },
    /// a cheat file as Citra and Azahar keep them.
    Cheats { path: PathBuf, title_id: Option<u64>, count: usize },
    /// something Zakuro can't use, and why.
    Unusable { path: PathBuf, why: String },
}

impl Found {
    pub fn path(&self) -> &Path {
        match self {
            Found::Title { path, .. }
            | Found::ZippedGames { path, .. }
            | Found::Textures { path, .. }
            | Found::Cheats { path, .. }
            | Found::Unusable { path, .. } => path,
        }
    }
}

/// what the paths are, looking inside folders and zips.
pub fn identify(paths: &[PathBuf]) -> Vec<Found> {
    let mut found = Vec::new();
    for path in paths {
        identify_one(path, &mut found);
    }
    found
}

fn identify_one(path: &Path, found: &mut Vec<Found>) {
    let ending = path.extension().and_then(|ending| ending.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if path.is_dir() {
        let mut games = Vec::new();
        library::game_files(path, 3, &mut games);
        if !games.is_empty() {
            for game in games {
                found.push(title(&game));
            }
            return;
        }
        let (images, size) = pictures(path, 0);
        if images > 0 {
            found.push(Found::Textures { path: path.to_owned(), name: file_name(path), zip: false, images, size, title_id: title_id_in(path) });
        } else {
            found.push(Found::Unusable { path: path.to_owned(), why: "No games, pictures or cheats in it".to_owned() });
        }
    } else if library::is_game_file(path) {
        found.push(title(path));
    } else if ending == "zip" {
        found.push(zip(path));
    } else if ending == "txt" {
        let cheats = std::fs::read_to_string(path).map(|text| zakuro_core::cheats::parse(&text)).unwrap_or_default();
        // any text parses as a cheat, the ones with codes are
        let real = cheats.iter().filter(|cheat| cheat.lines().iter().any(|line| zakuro_core::cheats::is_code(line))).count();
        match real {
            0 => found.push(Found::Unusable { path: path.to_owned(), why: "No cheats in it".to_owned() }),
            count => found.push(Found::Cheats { path: path.to_owned(), title_id: title_id_in(path), count }),
        }
    } else {
        found.push(Found::Unusable { path: path.to_owned(), why: "Not a game, a texture pack or cheats".to_owned() });
    }
}

/// a game file's title.
fn title(path: &Path) -> Found {
    let size = std::fs::metadata(path).map_or(0, |metadata| metadata.len());
    let unusable = |why: &str| Found::Unusable { path: path.to_owned(), why: why.to_owned() };
    // updates and DLC are CIAs with no program of their own to load
    if path.extension().is_some_and(|ending| ending.eq_ignore_ascii_case("cia")) {
        let Ok(image) = zakuro_fs::RomImage::open(path) else { return unusable("Can't be read") };
        let Ok(cia) = zakuro_fs::Cia::read(image.data()) else { return unusable("Not a CIA Zakuro can read") };
        if cia.kind() != TitleKind::Game {
            if cia.contents.iter().any(|content| content.offset.is_some() && content.encrypted()) {
                return unusable("Encrypted, Zakuro needs a decrypted dump");
            }
            let name = match cia.kind() {
                TitleKind::Update => format!("Update {}", zakuro_fs::version_name(cia.version)),
                TitleKind::Dlc => "DLC".to_owned(),
                _ => "Download Play program".to_owned(),
            };
            return Found::Title { path: path.to_owned(), title_id: cia.title_id, kind: cia.kind(), version: cia.version, name, size };
        }
    }
    match zakuro_fs::Title::load(path) {
        Ok(title) => {
            let name = title
                .exefs_file("icon")
                .and_then(library::Smdh::parse)
                .map(|smdh| smdh.name)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| file_name(path));
            Found::Title { path: path.to_owned(), title_id: title.program_id(), kind: TitleKind::Game, version: 0, name, size }
        }
        Err(zakuro_fs::FsError::Encrypted(_) | zakuro_fs::FsError::EncryptedCia) => unusable("Encrypted, Zakuro needs a decrypted dump"),
        Err(_) => unusable("Not a game Zakuro can read"),
    }
}

/// what a zip holds: games, or a texture pack.
fn zip(path: &Path) -> Found {
    let unusable = |why: &str| Found::Unusable { path: path.to_owned(), why: why.to_owned() };
    let Ok(mut archive) = File::open(path).map_err(|_| ()).and_then(|file| zip::ZipArchive::new(file).map_err(|_| ())) else {
        return unusable("Not a zip Zakuro can open");
    };
    let (mut games, mut games_size, mut images, mut images_size) = (0, 0, 0, 0);
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(index) else { continue };
        let Some(name) = entry.enclosed_name() else { continue };
        if library::is_game_file(&name) {
            games += 1;
            games_size += entry.size();
        } else if is_picture(&name) {
            images += 1;
            images_size += entry.size();
        }
    }
    if games > 0 {
        Found::ZippedGames { path: path.to_owned(), files: games, size: games_size }
    } else if images > 0 {
        Found::Textures { path: path.to_owned(), name: file_name(path), zip: true, images, size: images_size, title_id: title_id_in(path) }
    } else {
        unusable("No games or pictures in the zip")
    }
}

fn is_picture(path: &Path) -> bool {
    path.extension().is_some_and(|ending| ending.eq_ignore_ascii_case("png"))
}

/// how many pictures a folder and those in it hold, and their size.
fn pictures(folder: &Path, depth: u32) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(folder) else { return (0, 0) };
    let (mut count, mut size) = (0, 0);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 8 {
                let (more, bytes) = pictures(&path, depth + 1);
                count += more;
                size += bytes;
            }
        } else if is_picture(&path) {
            count += 1;
            size += entry.metadata().map_or(0, |metadata| metadata.len());
        }
    }
    (count, size)
}

/// a title id in a path's names, as Citra names a pack's folder and a
/// cheat file by the game's.
pub fn title_id_in(path: &Path) -> Option<u64> {
    let names = path.components().rev().filter_map(|part| part.as_os_str().to_str()).map(|name| name.split('.').next().unwrap_or(name).to_owned());
    let mut also = Vec::new();
    // a pack whose folder holds its game's folder, load/textures/<id>
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            also.extend(entries.flatten().filter(|entry| entry.path().is_dir()).filter_map(|entry| entry.file_name().to_str().map(str::to_owned)));
        }
    }
    names.chain(also).find_map(|name| {
        let id = u64::from_str_radix(name.trim(), 16).ok().filter(|_| name.trim().len() == 16)?;
        (id >> 32 == 0x0004_0000).then_some(id)
    })
}

fn file_name(path: &Path) -> String {
    path.file_stem().or(path.file_name()).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// a texture pack installed for a game.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPack {
    pub name: String,
    /// where it is in the game's mods folder.
    pub path: PathBuf,
    /// where it really is, when the game has only a link to it.
    pub target: Option<PathBuf>,
}

/// the texture packs a game has, a folder each, and the pictures put in
/// its textures folder loose as one more.
pub fn texture_packs(data_dir: &Path, program_id: u64) -> Vec<InstalledPack> {
    let dir = zakuro_core::mods::textures(data_dir, program_id);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut packs = Vec::new();
    let mut loose = false;
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            let target = std::fs::read_link(&path).ok();
            packs.push(InstalledPack { name: file_name_full(&path), path, target });
        } else if is_picture(&path) || path.file_name().is_some_and(|name| name == "pack.json") {
            loose = true;
        }
    }
    if loose {
        packs.push(InstalledPack { name: "Loose pictures".to_owned(), path: dir, target: None });
    }
    packs
}

fn file_name_full(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// a game's files in the folders the library lists: the game, and every
/// update, DLC and Download Play child of it, not only the ones in use.
pub fn game_files(game: &library::Game, sources: &[Source]) -> Vec<PathBuf> {
    let mut files = vec![game.path.clone()];
    let mut found = Vec::new();
    for source in sources {
        library::game_files(&source.folder, source.depth, &mut found);
    }
    for path in found {
        let ours = matches!(library::addon(&path), Some(Ok(addon)) if addon.title_id & 0xFFFF_FFFF == game.program_id & 0xFFFF_FFFF);
        if ours && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}

/// takes a game out of the library: its files there are deleted, with the
/// game's folder once it is empty, and the number deleted returned. a file
/// outside the library is never touched. saves and mods stay.
pub fn remove_game(files: &[PathBuf], library: &Path) -> std::io::Result<usize> {
    let mut deleted = 0;
    for file in files.iter().filter(|file| file.starts_with(library)) {
        std::fs::remove_file(file)?;
        deleted += 1;
        // a folder a game was unzipped into goes with its last file
        let mut folder = file.parent();
        while let Some(dir) = folder.filter(|dir| *dir != library && dir.starts_with(library)) {
            if std::fs::remove_dir(dir).is_err() {
                break;
            }
            folder = dir.parent();
        }
    }
    Ok(deleted)
}

/// takes a texture pack out of a game, the link to it alone when it is
/// one, the pack staying where it was.
pub fn remove_pack(pack: &InstalledPack) -> std::io::Result<()> {
    if pack.target.is_some() {
        // a junction or symlink to a folder goes as an empty folder does
        return std::fs::remove_dir(&pack.path).or_else(|_| std::fs::remove_file(&pack.path));
    }
    if pack.name == "Loose pictures" {
        for entry in std::fs::read_dir(&pack.path)?.flatten() {
            let path = entry.path();
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
        return Ok(());
    }
    std::fs::remove_dir_all(&pack.path)
}

/// one thing an install does.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// copies a file, unless the same is there.
    Copy { from: PathBuf, to: PathBuf },
    /// copies a folder and what is in it.
    CopyTree { from: PathBuf, to: PathBuf },
    /// unzips the games, or the pictures, of a zip into a folder.
    Unzip { from: PathBuf, to: PathBuf, games: bool },
    /// links a folder in from where it is.
    Link { from: PathBuf, to: PathBuf },
    /// adds a cheat file's cheats to a game's, leaving the ones it has.
    Cheats { from: PathBuf, to: PathBuf },
}

/// where a texture pack is put, what the player chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackPlace {
    /// copied into Zakuro, which keeps working with the pack's drive gone.
    #[default]
    Copy,
    /// used from where it is, taking no more room.
    InPlace,
}

/// what the player chose to add: the things found, each with the games it
/// goes to, for those that go to a game.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Choice {
    pub found: Vec<(Found, Vec<u64>)>,
    pub place: PackPlace,
}

/// the steps that add what was chosen, with how many bytes they move.
pub fn plan(choice: &Choice, library: &Path, data_dir: &Path) -> (Vec<Step>, u64) {
    let mut steps = Vec::new();
    let mut bytes = 0;
    for (found, games) in &choice.found {
        match found {
            Found::Title { path, size, .. } => {
                // a file already in the library stays where it is
                if !path.starts_with(library) {
                    steps.push(Step::Copy { from: path.clone(), to: library.join(path.file_name().unwrap_or_default()) });
                    bytes += size;
                }
            }
            Found::ZippedGames { path, size, .. } => {
                steps.push(Step::Unzip { from: path.clone(), to: library.join(file_name(path)), games: true });
                bytes += size;
            }
            Found::Textures { path, name, zip, size, .. } if !games.is_empty() => {
                let folder_name = sanitize(name);
                // copied once, then linked into each game
                let source = match (choice.place, zip) {
                    (PackPlace::InPlace, false) => path.clone(),
                    (_, true) => {
                        let to = shared_textures(data_dir).join(&folder_name);
                        steps.push(Step::Unzip { from: path.clone(), to: to.clone(), games: false });
                        bytes += size;
                        to
                    }
                    (PackPlace::Copy, false) => {
                        let to = shared_textures(data_dir).join(&folder_name);
                        steps.push(Step::CopyTree { from: path.clone(), to: to.clone() });
                        bytes += size;
                        to
                    }
                };
                for &game in games {
                    let to = zakuro_core::mods::textures(data_dir, game).join(&folder_name);
                    steps.push(Step::Link { from: source.clone(), to });
                }
            }
            Found::Cheats { path, .. } => {
                for &game in games {
                    steps.push(Step::Cheats { from: path.clone(), to: zakuro_core::cheats::path(data_dir, game) });
                }
            }
            Found::Textures { .. } | Found::Unusable { .. } => {}
        }
    }
    (steps, bytes)
}

/// a name that makes a folder anywhere.
fn sanitize(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_().,'&+".contains(c) { c } else { '_' }).collect();
    match clean.trim() {
        "" => "Texture pack".to_owned(),
        clean => clean.to_owned(),
    }
}

/// how an install is getting on.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// what it is doing now.
    pub doing: String,
    pub finished: bool,
    /// what went wrong, a line each.
    pub errors: Vec<String>,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            if self.finished { 1.0 } else { 0.0 }
        } else {
            (self.done as f64 / self.total as f64).min(1.0) as f32
        }
    }
}

/// an install running in the background.
pub struct Install {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl Install {
    pub fn start(steps: Vec<Step>, total: u64) -> Install {
        let progress = Arc::new(Mutex::new(Progress { total, ..Progress::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared, stop) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let mut runner = Runner { progress: shared.clone(), cancel: stop };
            for step in steps {
                if runner.cancelled() {
                    break;
                }
                if let Err(error) = runner.run(&step) {
                    log::warn!("installing failed, {step:?}, {error}");
                    lock(&shared).errors.push(error);
                }
            }
            let mut progress = lock(&shared);
            if runner.cancelled() {
                progress.errors.push("Stopped before it finished".to_owned());
            }
            progress.finished = true;
        });
        Install { progress, cancel }
    }

    pub fn progress(&self) -> Progress {
        lock(&self.progress).clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn lock(progress: &Mutex<Progress>) -> std::sync::MutexGuard<'_, Progress> {
    progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Runner {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl Runner {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn doing(&self, what: String) {
        lock(&self.progress).doing = what;
    }

    fn moved(&self, bytes: u64) {
        lock(&self.progress).done += bytes;
    }

    fn run(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::Copy { from, to } => {
                self.doing(format!("Copying {}", file_name_full(from)));
                let same = std::fs::metadata(to).ok().zip(std::fs::metadata(from).ok()).is_some_and(|(there, here)| there.len() == here.len());
                if same {
                    self.moved(std::fs::metadata(from).map_or(0, |metadata| metadata.len()));
                    return Ok(());
                }
                self.copy_file(from, to)
            }
            Step::CopyTree { from, to } => {
                self.doing(format!("Copying {}", file_name_full(from)));
                self.copy_tree(from, to, 0)
            }
            Step::Unzip { from, to, games } => {
                self.doing(format!("Unzipping {}", file_name_full(from)));
                self.unzip(from, to, *games)
            }
            Step::Link { from, to } => {
                self.doing(format!("Linking {}", file_name_full(from)));
                link(from, to)
            }
            Step::Cheats { from, to } => {
                self.doing(format!("Adding cheats from {}", file_name_full(from)));
                let new = zakuro_core::cheats::load(from);
                let mut cheats = zakuro_core::cheats::load(to);
                for cheat in new {
                    if !cheats.iter().any(|known| known.name == cheat.name) {
                        cheats.push(cheat);
                    }
                }
                if let Some(folder) = to.parent() {
                    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
                }
                zakuro_core::cheats::save(to, &cheats).map_err(|error| error.to_string())
            }
        }
    }

    /// copies a file through a part file, so that one stopped half way
    /// never passes for the whole.
    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        let fail = |error: std::io::Error| format!("{}: {error}", file_name_full(from));
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(fail)?;
        }
        let part = to.with_extension(format!("{}.part", to.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let mut reader = File::open(from).map_err(fail)?;
        let mut writer = File::create(&part).map_err(fail)?;
        let mut buffer = vec![0; 4 << 20];
        loop {
            if self.cancelled() {
                drop(writer);
                let _ = std::fs::remove_file(&part);
                return Ok(());
            }
            let read = reader.read(&mut buffer).map_err(fail)?;
            if read == 0 {
                break;
            }
            writer.write_all(&buffer[..read]).map_err(fail)?;
            self.moved(read as u64);
        }
        writer.flush().map_err(fail)?;
        drop(writer);
        std::fs::rename(&part, to).map_err(fail)
    }

    fn copy_tree(&mut self, from: &Path, to: &Path, depth: u32) -> Result<(), String> {
        let fail = |error: std::io::Error| format!("{}: {error}", from.display());
        std::fs::create_dir_all(to).map_err(fail)?;
        for entry in std::fs::read_dir(from).map_err(fail)?.flatten() {
            if self.cancelled() {
                return Ok(());
            }
            let path = entry.path();
            let into = to.join(entry.file_name());
            if path.is_dir() {
                if depth < 16 {
                    self.copy_tree(&path, &into, depth + 1)?;
                }
            } else if is_picture(&path) || path.file_name().is_some_and(|name| name == "pack.json") {
                let same = std::fs::metadata(&into).ok().zip(entry.metadata().ok()).is_some_and(|(there, here)| there.len() == here.len());
                if same {
                    self.moved(entry.metadata().map_or(0, |metadata| metadata.len()));
                } else {
                    std::fs::copy(&path, &into).map_err(fail)?;
                    self.moved(entry.metadata().map_or(0, |metadata| metadata.len()));
                }
            }
        }
        Ok(())
    }

    fn unzip(&mut self, from: &Path, to: &Path, games: bool) -> Result<(), String> {
        let fail = |error: String| format!("{}: {error}", file_name_full(from));
        let file = File::open(from).map_err(|error| fail(error.to_string()))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|error| fail(error.to_string()))?;
        for index in 0..archive.len() {
            if self.cancelled() {
                return Ok(());
            }
            let mut entry = archive.by_index(index).map_err(|error| fail(error.to_string()))?;
            let Some(name) = entry.enclosed_name() else { continue };
            let wanted = if games { library::is_game_file(&name) } else { is_picture(&name) || name.file_name().is_some_and(|n| n == "pack.json") };
            if entry.is_dir() || !wanted {
                continue;
            }
            // games go in flat, pictures keep their folders
            let into = if games { to.join(name.file_name().unwrap_or_default()) } else { to.join(&name) };
            if let Some(folder) = into.parent() {
                std::fs::create_dir_all(folder).map_err(|error| fail(error.to_string()))?;
            }
            let mut writer = File::create(&into).map_err(|error| fail(error.to_string()))?;
            let mut buffer = vec![0; 1 << 20];
            loop {
                let read = entry.read(&mut buffer).map_err(|error| fail(error.to_string()))?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read]).map_err(|error| fail(error.to_string()))?;
                self.moved(read as u64);
            }
        }
        Ok(())
    }
}

/// links the folder to from where it is, a junction on Windows, which
/// needs no administrator, and a symbolic link elsewhere.
fn link(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    if std::fs::symlink_metadata(to).is_ok() {
        // a link there already goes, for the new one; a real folder stays
        if std::fs::read_link(to).is_ok() {
            std::fs::remove_dir(to).or_else(|_| std::fs::remove_file(to)).map_err(|error| error.to_string())?;
        } else {
            return Err(format!("{} is there already", to.display()));
        }
    }
    let from = from.canonicalize().map_err(|error| format!("{}: {error}", from.display()))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // \\?\ paths don't go through mklink
        let plain = |path: &Path| path.display().to_string().trim_start_matches(r"\\?\").to_owned();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &plain(to), &plain(&from)])
            .creation_flags(0x0800_0000) // no console window
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("Couldn't link {} into {}", from.display(), to.display())) }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&from, to).map_err(|error| error.to_string())
    }
}

/// a size as people read them.
pub fn size_name(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1024)),
    }
}

/// the games a pack is likely for, by its title id, or by the words of its
/// name the games' names share, which the player checks before adding it.
pub fn likely_games(found: &Found, games: &[(u64, String)]) -> Vec<u64> {
    let (name, id) = match found {
        Found::Textures { name, title_id, .. } => (name.as_str(), *title_id),
        Found::Cheats { path, title_id, .. } => (path.file_stem().and_then(|s| s.to_str()).unwrap_or(""), *title_id),
        _ => return Vec::new(),
    };
    if let Some(id) = id {
        if games.iter().any(|(game, _)| *game == id) {
            return vec![id];
        }
    }
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric()).filter(|word| word.len() > 2).map(str::to_lowercase).filter(|word| !["the", "and", "textures", "texture", "pack", "pokemon", "pokémon"].contains(&word.as_str())).collect()
    };
    let wanted = words(name);
    games
        .iter()
        .filter(|(_, game)| words(game).iter().any(|word| wanted.contains(word)))
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zakuro-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// a game's copies in the library go, with the folder they were in once
    /// empty, and files elsewhere stay.
    #[test]
    fn removing_a_game_deletes_only_the_library_copies() {
        let dir = temp("remove");
        let library = dir.join("library");
        let own = library.join("Some game").join("game.3ds");
        let update = library.join("update.cia");
        let elsewhere = dir.join("mine").join("dlc.cia");
        for file in [&own, &update, &elsewhere] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        }
        let deleted = remove_game(&[own.clone(), update.clone(), elsewhere.clone()], &library).unwrap();
        assert_eq!(deleted, 2);
        assert!(!own.exists() && !update.exists() && !library.join("Some game").exists());
        assert!(library.exists() && elsewhere.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn title_ids_are_found_in_names() {
        assert_eq!(title_id_in(Path::new("load/textures/00040000001B5000")), Some(0x0004_0000_001B_5000));
        assert_eq!(title_id_in(Path::new("cheats/00040000001b5000.txt")), Some(0x0004_0000_001B_5000));
        assert_eq!(title_id_in(Path::new("HD Textures")), None);
        // sixteen digits that aren't a game's id
        assert_eq!(title_id_in(Path::new("0D80898632A22ABD")), None);
    }

    #[test]
    fn a_folder_of_pictures_is_a_texture_pack() {
        let dir = temp("pack");
        std::fs::create_dir_all(dir.join("HD Textures")).unwrap();
        std::fs::write(dir.join("HD Textures/tex1_128x128_0D80898632A22ABD_12.png"), [0; 10]).unwrap();
        std::fs::write(dir.join("readme.txt"), "hi").unwrap();
        let found = identify(std::slice::from_ref(&dir));
        assert!(matches!(&found[..], [Found::Textures { images: 1, size: 10, zip: false, .. }]), "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cheat_files_and_strangers_are_told_apart() {
        let dir = temp("cheats");
        let cheats = dir.join("00040000001B5000.txt");
        std::fs::write(&cheats, "[No Outlines]\n0042FBA4 E320F000\n").unwrap();
        let other = dir.join("notes.txt");
        std::fs::write(&other, "nothing here").unwrap();
        let found = identify(&[cheats, other]);
        assert!(matches!(found[0], Found::Cheats { title_id: Some(0x0004_0000_001B_5000), count: 1, .. }), "{found:?}");
        assert!(matches!(found[1], Found::Unusable { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pack_for_two_games_is_copied_once_and_linked_twice() {
        let pack = Found::Textures { path: "D:/packs/HD".into(), name: "HD".into(), zip: false, images: 3, size: 300, title_id: None };
        let choice = Choice { found: vec![(pack, vec![1, 2])], place: PackPlace::Copy };
        let (steps, bytes) = plan(&choice, Path::new("L"), Path::new("Z"));
        assert_eq!(bytes, 300);
        assert_eq!(steps.iter().filter(|step| matches!(step, Step::CopyTree { .. })).count(), 1);
        assert_eq!(steps.iter().filter(|step| matches!(step, Step::Link { .. })).count(), 2);
        // used in place, nothing is copied
        let choice = Choice { place: PackPlace::InPlace, ..choice };
        let (steps, bytes) = plan(&choice, Path::new("L"), Path::new("Z"));
        assert_eq!(bytes, 0);
        assert!(steps.iter().all(|step| matches!(step, Step::Link { .. })));
    }

    #[test]
    fn games_already_in_the_library_stay_put() {
        let game = |path: &str| Found::Title { path: path.into(), title_id: 1, kind: TitleKind::Game, version: 0, name: "A".into(), size: 5 };
        let choice = Choice { found: vec![(game("L/a.3ds"), vec![]), (game("D/b.3ds"), vec![])], place: PackPlace::Copy };
        let (steps, bytes) = plan(&choice, Path::new("L"), Path::new("Z"));
        assert_eq!(steps, [Step::Copy { from: "D/b.3ds".into(), to: Path::new("L").join("b.3ds") }]);
        assert_eq!(bytes, 5);
    }

    #[test]
    fn packs_are_matched_to_games_by_name() {
        let games = [(1, "Pokémon Ultra Sun".to_owned()), (2, "Pokémon Ultra Moon".to_owned()), (3, "Super Mario 3D Land".to_owned())];
        let pack = Found::Textures { path: "x".into(), name: "Ultra Sun HD".into(), zip: false, images: 1, size: 1, title_id: None };
        assert_eq!(likely_games(&pack, &games), [1, 2]);
        let pack = Found::Textures { path: "x".into(), name: "whatever".into(), zip: false, images: 1, size: 1, title_id: Some(3) };
        assert_eq!(likely_games(&pack, &games), [3]);
    }

    #[test]
    fn copying_installs_and_reports_progress() {
        let dir = temp("copy");
        let from = dir.join("a.3ds");
        std::fs::write(&from, vec![7; 1000]).unwrap();
        let to = dir.join("library/a.3ds");
        let install = Install::start(vec![Step::Copy { from, to: to.clone() }], 1000);
        let start = std::time::Instant::now();
        while !install.progress().finished && start.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::yield_now();
        }
        let progress = install.progress();
        assert!(progress.errors.is_empty(), "{:?}", progress.errors);
        assert_eq!(progress.done, 1000);
        assert_eq!(std::fs::read(&to).unwrap().len(), 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_name(2_900_000_000), "2.7 GB");
        assert_eq!(size_name(5 << 20), "5 MB");
        assert_eq!(size_name(1500), "2 KB");
    }
}
