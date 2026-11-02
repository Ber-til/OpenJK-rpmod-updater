//! The updater's own releases: a newer one downloaded and put where this program is, then started
//! in its place.
//!
//! - Windows: the running exe can't be overwritten but can be renamed, so it becomes
//!   `<name>.exe.old` (removed by the next start) and the new one takes its name.
//! - Linux: the new binary is renamed over the running one.
//! - macOS: what's in the new app's Contents is renamed over this app's.

use crate::github::{self, Release};
use crate::install::{self, Failure, Step};
use crate::version::Version;
use std::fs;
use std::path::{Path, PathBuf};

pub const REPO: &str = "Ber-til/OpenJK-rpmod-updater";
pub const RELEASES_PAGE: &str = "https://github.com/Ber-til/OpenJK-rpmod-updater/releases/latest";

/// the release tag when GitHub Actions built it for one, otherwise Cargo.toml's version
pub const VERSION: &str = match option_env!("UPDATER_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => concat!("v", env!("CARGO_PKG_VERSION")),
};

/// the updater's download for this computer
pub fn asset_name() -> &'static str {
    if cfg!(windows) {
        "OpenJK-rpmod-Updater.exe"
    } else if cfg!(target_os = "macos") {
        "OpenJK-rpmod-Updater-macos.zip"
    } else {
        "OpenJK-rpmod-Updater-linux.tar.gz"
    }
}

/// the newest release of the updater, when it's newer than this one and has a download for here
pub fn newer(releases: &[Release]) -> Option<Release> {
    let current = Version::parse(VERSION)?;
    let newest = releases.first()?;
    (current.is_older_than(&newest.version) && newest.asset_named(asset_name()).is_some()).then(|| newest.clone())
}

fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

fn new_path(target: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".new");
    target.with_file_name(name)
}

/// what an update left behind, removed once the old program has closed
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = old_path(&exe);
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..40 {
            if fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

/// `release` put in place of this program; what to start afterwards is returned
pub fn update(release: &Release, step: impl FnMut(Step)) -> Result<PathBuf, Failure> {
    let exe = std::env::current_exe().map_err(|e| Failure::Other(format!("Couldn't tell where the updater is ({e})")))?;
    // through any links, to the file itself (on Windows it would turn into a \\?\ path)
    #[cfg(unix)]
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    update_at(release, exe, step)
}

/// `release` put in place of the updater at `exe`
fn update_at(release: &Release, exe: PathBuf, mut step: impl FnMut(Step)) -> Result<PathBuf, Failure> {
    let asset = release
        .asset_named(asset_name())
        .ok_or_else(|| Failure::Other(format!("The updater's {} has no download for this computer", release.version)))?;
    let dir = exe.parent().ok_or_else(|| Failure::Other("Couldn't tell where the updater is".into()))?;
    install::check_writable(dir)?;

    let download = std::env::temp_dir().join(format!("openjk-rpmod-updater-{}-{}", release.version, asset.name));
    github::download(asset, &download, |done| step(Step::Downloading { done, total: asset.size })).map_err(Failure::Other)?;
    step(Step::Unpacking { done: 0, total: 1 });
    let result = put_in_place(&download, &exe);
    let _ = fs::remove_file(&download);
    step(Step::Unpacking { done: 1, total: 1 });
    result.map(|()| exe)
}

/// `to` written as a file next to it, then renamed over it, so it's never half written
#[cfg(unix)]
fn replace(to: &Path, write: impl FnOnce(&Path) -> std::io::Result<()>, mode: u32) -> Result<(), Failure> {
    use std::os::unix::fs::PermissionsExt;
    let temp = new_path(to);
    write(&temp).map_err(|e| install::io_failure(&temp, e))?;
    let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(mode));
    fs::rename(&temp, to).map_err(|e| {
        let _ = fs::remove_file(&temp);
        install::io_failure(to, e)
    })
}

#[cfg(windows)]
fn put_in_place(download: &Path, exe: &Path) -> Result<(), Failure> {
    let new = new_path(exe);
    let old = old_path(exe);
    fs::copy(download, &new).map_err(|e| install::io_failure(&new, e))?;
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(exe, &old) {
        let _ = fs::remove_file(&new);
        return Err(install::io_failure(exe, e));
    }
    if let Err(e) = fs::rename(&new, exe) {
        let _ = fs::rename(&old, exe);
        let _ = fs::remove_file(&new);
        return Err(install::io_failure(exe, e));
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn put_in_place(download: &Path, exe: &Path) -> Result<(), Failure> {
    let file = fs::File::open(download).map_err(|e| install::io_failure(download, e))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(install::damaged)? {
        let mut entry = entry.map_err(install::damaged)?;
        let is_program = entry.header().entry_type().is_file()
            && entry.path().ok().is_some_and(|p| p.file_name().is_some_and(|n| n == "OpenJK-rpmod-Updater"));
        if is_program {
            return replace(exe, |temp| entry.unpack(temp).map(|_| ()), 0o755);
        }
    }
    Err(Failure::Other("The download has no updater in it".into()))
}

#[cfg(target_os = "macos")]
fn put_in_place(download: &Path, exe: &Path) -> Result<(), Failure> {
    use std::io::Read;
    const PROGRAM: &str = "Contents/MacOS/openjk-rpmod-updater";

    let file = fs::File::open(download).map_err(|e| install::io_failure(download, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Failure::Other(format!("The download is damaged ({e})")))?;
    // this app's Contents folder, when the updater is run from its app
    let contents = exe
        .parent()
        .and_then(Path::parent)
        .filter(|c| c.file_name().is_some_and(|n| n == "Contents"))
        .filter(|c| c.parent().and_then(Path::extension).is_some_and(|x| x == "app"))
        .map(Path::to_path_buf);

    let mut found = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Failure::Other(format!("The download is damaged ({e})")))?;
        let Ok(name) = entry.name().map(|n| n.replace('\\', "/")) else { continue };
        if entry.is_dir() || name.starts_with("__MACOSX/") || name.rsplit('/').next().is_some_and(|n| n.starts_with("._")) {
            continue;
        }
        let Some(at) = name.find("/Contents/") else { continue };
        let inside = &name[at + 1..];
        let target = match &contents {
            Some(contents) => {
                let Some(rel) = install::safe_relative(Path::new(&inside["Contents/".len()..])) else { continue };
                contents.join(rel)
            }
            None if inside == PROGRAM => exe.to_path_buf(),
            None => continue,
        };
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(install::damaged)?;
        let mode = if inside == PROGRAM { 0o755 } else { entry.unix_mode().unwrap_or(0o644) & 0o777 };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| install::io_failure(parent, e))?;
        }
        replace(&target, |temp| fs::write(temp, &data), mode)?;
        found |= inside == PROGRAM;
    }
    if found { Ok(()) } else { Err(Failure::Other("The download has no updater in it".into())) }
}

/// the updated program started, with the arguments this one had
pub fn start(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("The new updater couldn't be started ({e}). Open it again yourself."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the newest updater put in place of a made-up one, the old one kept aside on Windows
    /// (cargo test -- --ignored: it downloads)
    #[test]
    #[ignore]
    fn replaces_itself() {
        let dir = std::env::temp_dir().join("openjk-rpmod-updater-self-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(if cfg!(windows) { "OpenJK-rpmod-Updater.exe" } else { "OpenJK-rpmod-Updater" });
        fs::write(&exe, b"the old updater").unwrap();

        let releases = github::releases_of(REPO).unwrap();
        let newest = &releases[0];
        let started = update_at(newest, exe.clone(), |_| {}).unwrap_or_else(|_| panic!("update failed"));
        assert_eq!(started, exe);
        let now = fs::read(&exe).unwrap();
        assert!(now.len() > 1_000_000, "{} bytes", now.len());
        if cfg!(windows) {
            assert_eq!(newest.asset_named(asset_name()).unwrap().size, now.len() as u64);
            assert_eq!(fs::read(old_path(&exe)).unwrap(), b"the old updater");
        }
        assert!(!new_path(&exe).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
