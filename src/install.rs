//! Putting a release into GameData: downloaded, then unpacked over what's there. The first time,
//! the game's own files it replaces are kept in GameData/OpenJK-rpmod-backup.

use crate::github::{self, Release};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const BACKUP_DIR: &str = "OpenJK-rpmod-backup";
/// in the 0.7.3 zip, and never wanted now
const SKIPPED: &[&str] = &["Install-OpenJK-rpmod.bat"];

pub enum Step {
    Downloading { done: u64, total: u64 },
    Unpacking { done: usize, total: usize },
}

pub enum Failure {
    /// GameData can't be written to without administrator rights
    NeedsAdmin(String),
    Other(String),
}

pub(crate) fn io_failure(path: &Path, e: io::Error) -> Failure {
    let message = format!("{}: {e}", path.display());
    if e.kind() == io::ErrorKind::PermissionDenied {
        Failure::NeedsAdmin(message)
    } else {
        Failure::Other(message)
    }
}

/// the game running: its files can't be replaced
pub fn game_running() -> bool {
    use sysinfo::{ProcessesToUpdate, System, get_current_pid};

    let Ok(mut system) = System::new() else { return false };
    let me = get_current_pid().ok();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.processes().iter().any(|(pid, p)| {
        Some(*pid) != me && is_game(&p.name().to_string_lossy(), p.exe().and_then(|e| e.file_name()).map(|f| f.to_string_lossy()).as_deref())
    })
}

/// a process that's the game (openjk.x86, openjk.x86_64, openjk.arm64...), by its program's file
/// name where it's known, else by its name. Never the updater: Linux cuts a process's name to 15
/// letters, so OpenJK-rpmod-Updater is "OpenJK-rpmod-Up" there, and nothing of the game is called
/// openjk-rpmod
fn is_game(name: &str, exe: Option<&str>) -> bool {
    let name = exe.unwrap_or(name).to_lowercase();
    name.starts_with("openjk") && !name.starts_with("openjk-rpmod") && !name.contains("updater")
}

/// whether GameData takes new files
pub(crate) fn check_writable(game_data: &Path) -> Result<(), Failure> {
    let probe = game_data.join(".openjk-rpmod-updater-probe");
    fs::write(&probe, b"").map_err(|e| io_failure(game_data, e))?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

/// a path from the archive, only when it stays inside GameData
pub(crate) fn safe_relative(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn skipped(rel: &Path) -> bool {
    rel.file_name().is_some_and(|n| SKIPPED.iter().any(|s| n.eq_ignore_ascii_case(s)))
}

/// the game's file at `target` kept, the first time only
fn back_up(game_data: &Path, rel: &Path, target: &Path) -> Result<(), Failure> {
    if !target.is_file() {
        return Ok(());
    }
    let backup = game_data.join(BACKUP_DIR).join(rel);
    if backup.exists() {
        return Ok(());
    }
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).map_err(|e| io_failure(parent, e))?;
    }
    fs::copy(target, &backup).map(|_| ()).map_err(|e| io_failure(&backup, e))
}

/// `release` into `game_data`; `first`: OpenJK-rpmod isn't there yet, so what it replaces is kept
pub fn install(release: &Release, game_data: &Path, first: bool, mut step: impl FnMut(Step)) -> Result<(), Failure> {
    let asset = release
        .asset()
        .ok_or_else(|| Failure::Other(format!("{} has no download for this computer ({})", release.version, crate::installs::asset_name())))?;
    check_writable(game_data)?;

    let archive = std::env::temp_dir().join(format!("openjk-rpmod-{}-{}", release.version, asset.name));
    github::download(asset, &archive, |done| step(Step::Downloading { done, total: asset.size })).map_err(Failure::Other)?;

    let result = if asset.name.ends_with(".zip") {
        unpack_zip(&archive, game_data, first, &mut step)
    } else {
        unpack_tar_gz(&archive, game_data, first, &mut step)
    };
    let _ = fs::remove_file(&archive);
    result
}

fn unpack_zip(archive: &Path, game_data: &Path, first: bool, step: &mut impl FnMut(Step)) -> Result<(), Failure> {
    let file = fs::File::open(archive).map_err(|e| io_failure(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Failure::Other(format!("The download is damaged ({e})")))?;
    let total = zip.len();

    for i in 0..total {
        let mut entry = zip.by_index(i).map_err(|e| Failure::Other(format!("The download is damaged ({e})")))?;
        step(Step::Unpacking { done: i, total });
        let Some(rel) = entry.enclosed_name().and_then(|p| safe_relative(&p)) else { continue };
        if skipped(&rel) {
            continue;
        }
        let target = game_data.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| io_failure(&target, e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_failure(parent, e))?;
        }
        if first {
            back_up(game_data, &rel, &target)?;
        }
        let mut out = fs::File::create(&target).map_err(|e| io_failure(&target, e))?;
        io::copy(&mut entry, &mut out).map_err(|e| io_failure(&target, e))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }
    step(Step::Unpacking { done: total, total });
    Ok(())
}

fn open_tar(archive: &Path) -> Result<tar::Archive<flate2::read::GzDecoder<fs::File>>, Failure> {
    let file = fs::File::open(archive).map_err(|e| io_failure(archive, e))?;
    Ok(tar::Archive::new(flate2::read::GzDecoder::new(file)))
}

pub(crate) fn damaged(e: io::Error) -> Failure {
    Failure::Other(format!("The download is damaged ({e})"))
}

fn unpack_tar_gz(archive: &Path, game_data: &Path, first: bool, step: &mut impl FnMut(Step)) -> Result<(), Failure> {
    let total = open_tar(archive)?.entries().map_err(damaged)?.count();
    let mut tar = open_tar(archive)?;
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);

    for (i, entry) in tar.entries().map_err(damaged)?.enumerate() {
        let mut entry = entry.map_err(damaged)?;
        step(Step::Unpacking { done: i, total });
        let Some(rel) = entry.path().ok().and_then(|p| safe_relative(&p)) else { continue };
        if skipped(&rel) {
            continue;
        }
        let target = game_data.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_failure(parent, e))?;
        }
        if first && entry.header().entry_type().is_file() {
            back_up(game_data, &rel, &target)?;
        }
        entry.unpack(&target).map_err(|e| io_failure(&target, e))?;
    }
    step(Step::Unpacking { done: total, total });
    Ok(())
}

/// this program again with administrator rights, to install into `game_data`
#[cfg(windows)]
pub fn restart_as_admin(game_data: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let quote = |s: &str| s.replace('\'', "''");
    let command = format!(
        "Start-Process -FilePath '{}' -ArgumentList @('--install', '\"{}\"') -Verb RunAs",
        quote(&exe.to_string_lossy()),
        quote(&game_data.to_string_lossy())
    );
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("Windows didn't give administrator rights".into()) }
}

#[cfg(not(windows))]
pub fn restart_as_admin(_game_data: &Path) -> Result<(), String> {
    Err("Run the updater with sudo to install there".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_is_found_and_the_updater_is_not() {
        assert!(is_game("openjk.x86.exe", None));
        assert!(is_game("openjk.x86_64", Some("openjk.x86_64")));
        assert!(is_game("openjk.arm64", None));
        assert!(is_game("openjkded.x86", None));
        // the updater: its whole name, Linux's 15 letters of it, or its program's name
        assert!(!is_game("OpenJK-rpmod-Updater.exe", None));
        assert!(!is_game("OpenJK-rpmod-Up", None));
        assert!(!is_game("OpenJK-rpmod-Up", Some("OpenJK-rpmod-Updater")));
        assert!(!is_game("bash", None));
        assert!(!is_game("steam", Some("steam")));
    }

    /// the newest release into a made-up GameData, the game's own file there kept
    /// (cargo test -- --ignored: it downloads)
    #[test]
    #[ignore]
    fn installs_newest_release() {
        let game_data = std::env::temp_dir().join("openjk-rpmod-updater-test");
        let _ = fs::remove_dir_all(&game_data);
        fs::create_dir_all(game_data.join("base")).unwrap();
        fs::write(game_data.join("base/assets0.pk3"), b"").unwrap();
        let engine = crate::installs::engine_path(&game_data);
        fs::create_dir_all(engine.parent().unwrap()).unwrap();
        fs::write(&engine, b"the game's own").unwrap();

        let releases = github::releases().unwrap();
        let newest = &releases[0];
        let mut steps = 0;
        if install(newest, &game_data, true, |_| steps += 1).is_err() {
            panic!("install failed");
        }

        let found = crate::installs::Install::read(&game_data);
        assert_eq!(found.version.as_ref().map(|v| v.nums), Some(newest.version.nums));
        let backup = game_data.join(BACKUP_DIR).join(engine.strip_prefix(&game_data).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"the game's own");
        assert!(!game_data.join("Install-OpenJK-rpmod.bat").exists());
        assert!(steps > 10);
        fs::remove_dir_all(&game_data).unwrap();
    }
}
