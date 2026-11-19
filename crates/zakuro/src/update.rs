//! Zakuro's own updates: the latest release on GitHub, whether it is newer
//! than this one, and, on Windows, putting its zakuro.exe in place of this
//! one, which then starts again as the new one.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// where Zakuro's releases are published.
const REPO: &str = "TVKain/zakuro";

/// this Zakuro's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// how long asking about the latest release may take, and downloading it.
const ASK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// a release newer than this Zakuro.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// its version, "0.3.8".
    pub version: String,
    /// its page, to download it from by hand.
    pub page: String,
    /// its Windows zip, which has zakuro.exe in it.
    pub windows_zip: Option<String>,
}

/// a version's numbers, "v0.3.8" or "0.3.8" as (0, 3, 8).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches(['v', 'V']).split('.').map(|part| part.parse().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// the release GitHub's answer about the latest one describes, when it is
/// newer than current.
pub fn newer_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let latest: serde_json::Value = serde_json::from_str(json).map_err(|error| format!("GitHub's answer can't be read: {error}"))?;
    let tag = latest["tag_name"].as_str().ok_or("GitHub's answer has no version")?;
    let (Some(version), Some(this)) = (parse_version(tag), parse_version(current)) else {
        return Err(format!("{tag} isn't a version Zakuro knows how to compare"));
    };
    if version <= this {
        return Ok(None);
    }
    let assets = latest["assets"].as_array().map(Vec::as_slice).unwrap_or_default();
    let windows_zip = assets.iter().find_map(|asset| {
        let name = asset["name"].as_str()?.to_ascii_lowercase();
        (name.contains("windows") && name.ends_with(".zip")).then(|| asset["browser_download_url"].as_str().map(str::to_owned))?
    });
    Ok(Some(Release {
        version: format!("{}.{}.{}", version.0, version.1, version.2),
        page: latest["html_url"].as_str().map_or_else(|| format!("https://github.com/{REPO}/releases/latest"), str::to_owned),
        windows_zip,
    }))
}

/// asks GitHub for the latest release, none when it is not newer.
pub fn check() -> Result<Option<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(ASK_TIMEOUT)).build().into();
    let mut response = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "zakuro-updater")
        .call()
        .map_err(|error| format!("GitHub can't be reached: {error}"))?;
    let json = response.body_mut().read_to_string().map_err(|error| format!("GitHub's answer can't be read: {error}"))?;
    newer_release(&json, CURRENT)
}

/// whether this Zakuro can put a release in place of itself: one on Windows
/// that isn't a build in a source folder's target folder.
pub fn can_install(release: &Release) -> bool {
    let built_here = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.file_name()? == "target")).unwrap_or(true);
    cfg!(windows) && !built_here && release.windows_zip.is_some()
}

/// what an update leaves of the Zakuro it replaced, beside the new one.
fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// downloads the release and puts its zakuro.exe where this one is, this
/// one moved aside, and says where the new one is, to start it.
pub fn install(release: &Release) -> Result<PathBuf, String> {
    let url = release.windows_zip.as_deref().ok_or("This release has no Windows download")?;
    let exe = std::env::current_exe().map_err(|error| format!("Zakuro can't tell where it is: {error}"))?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(DOWNLOAD_TIMEOUT)).build().into();
    let mut response = agent.get(url).header("User-Agent", "zakuro-updater").call().map_err(|error| format!("The update can't be downloaded: {error}"))?;
    let zip = response.body_mut().with_config().limit(256 * 1024 * 1024).read_to_vec().map_err(|error| format!("The update can't be downloaded: {error}"))?;
    let program = exe_in_zip(&zip)?;
    replace_running(&exe, &program)?;
    log::info!("updated to {} at {}", release.version, exe.display());
    Ok(exe)
}

/// zakuro.exe, out of a release's zip.
fn exe_in_zip(zip: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|error| format!("The update isn't a zip Zakuro can read: {error}"))?;
    let index = (0..archive.len())
        .find(|&index| {
            let Ok(file) = archive.by_index(index) else { return false };
            file.name().is_ok_and(|name| name.rsplit(['/', '\\']).next().is_some_and(|name| name.eq_ignore_ascii_case("zakuro.exe")))
        })
        .ok_or("The update has no zakuro.exe in it")?;
    let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
    let mut program = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut program).map_err(|error| format!("The update can't be unpacked: {error}"))?;
    Ok(program)
}

/// puts program where exe is, though exe is running: Windows lets a
/// running program be renamed but not overwritten, so it goes aside first,
/// and comes back if the new one can't be put there.
fn replace_running(exe: &Path, program: &[u8]) -> Result<(), String> {
    let fresh = exe.with_extension("new.exe");
    let old = old_exe(exe);
    let failed = |error: std::io::Error| format!("Zakuro can't update itself in {}: {error}", exe.parent().unwrap_or(exe).display());
    std::fs::write(&fresh, program).map_err(failed)?;
    let _ = std::fs::remove_file(&old);
    if let Err(error) = std::fs::rename(exe, &old) {
        let _ = std::fs::remove_file(&fresh);
        return Err(failed(error));
    }
    if let Err(error) = std::fs::rename(&fresh, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&fresh);
        return Err(failed(error));
    }
    Ok(())
}

/// deletes what the last update left of the Zakuro before it, which was
/// still running then.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let old = old_exe(&exe);
        if old.is_file() {
            match std::fs::remove_file(&old) {
                Ok(()) => log::info!("removed {} the last update left", old.display()),
                Err(error) => log::warn!("removing {}: {error}", old.display()),
            }
        }
    }
}

/// opens a web page in the browser.
pub fn open_page(url: &str) {
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else {
        let mut command = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
        command.arg(url);
        command
    };
    if let Err(error) = command.spawn() {
        log::warn!("opening {url}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(tag: &str) -> String {
        format!(
            r#"{{"tag_name": "{tag}", "html_url": "https://github.com/TVKain/zakuro/releases/tag/{tag}",
               "assets": [{{"name": "zakuro-setup-{tag}.exe", "browser_download_url": "https://example.invalid/setup.exe"}},
                          {{"name": "zakuro-{tag}-windows-x64.zip", "browser_download_url": "https://example.invalid/zakuro.zip"}}]}}"#
        )
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert_eq!(parse_version("v0.3.10"), Some((0, 3, 10)));
        assert_eq!(parse_version("0.3.7"), Some((0, 3, 7)));
        assert_eq!(parse_version("v0.3"), None);
        assert_eq!(parse_version("v0.3.7-beta"), None);
        assert!(parse_version("0.3.10") > parse_version("0.3.9"));
    }

    #[test]
    fn only_a_newer_release_is_offered_with_its_windows_zip() {
        let newer = newer_release(&answer("v0.3.10"), "0.3.9").unwrap().unwrap();
        assert_eq!(newer.version, "0.3.10");
        assert_eq!(newer.windows_zip.as_deref(), Some("https://example.invalid/zakuro.zip"), "the zip, not the setup");
        assert_eq!(newer.page, "https://github.com/TVKain/zakuro/releases/tag/v0.3.10");
        assert_eq!(newer_release(&answer("v0.3.9"), "0.3.9"), Ok(None), "the same one");
        assert_eq!(newer_release(&answer("v0.2.32"), "0.3.9"), Ok(None), "an older one");
        assert!(newer_release("not json", "0.3.9").is_err());
    }

    #[test]
    fn the_exe_comes_out_of_a_release_zip() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("README.md", options).unwrap();
        zip.write_all(b"readme").unwrap();
        zip.start_file("zakuro.exe", options).unwrap();
        zip.write_all(b"MZ new zakuro").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(exe_in_zip(&bytes).unwrap(), b"MZ new zakuro");
        assert!(exe_in_zip(b"not a zip").is_err());
    }

    /// GitHub answers about the latest release in a way this reads.
    #[test]
    #[ignore = "talks to GitHub"]
    fn github_answers_about_the_latest_release() {
        let release = check().expect("GitHub answers");
        println!("{release:?}");
        let pretend = newer_release(&ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest")).header("User-Agent", "zakuro-updater").call().unwrap().body_mut().read_to_string().unwrap(), "0.0.1");
        let pretend = pretend.unwrap().expect("newer than 0.0.1");
        // its zip has a zakuro.exe in it, a Windows program
        let url = pretend.windows_zip.expect("a Windows zip");
        let zip = ureq::get(&url).call().unwrap().body_mut().with_config().limit(256 * 1024 * 1024).read_to_vec().unwrap();
        let program = exe_in_zip(&zip).expect("zakuro.exe in the zip");
        assert!(program.starts_with(b"MZ") && program.len() > 1024 * 1024, "{} bytes", program.len());
    }

    /// the new program goes where the old one was, the old one aside.
    #[test]
    fn an_update_takes_the_old_programs_place() {
        let dir = std::env::temp_dir().join(format!("zakuro-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("zakuro.exe");
        std::fs::write(&exe, b"old").unwrap();
        replace_running(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(old_exe(&exe)).unwrap(), b"old");
        assert!(!exe.with_extension("new.exe").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
