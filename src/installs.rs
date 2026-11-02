//! Finding Jedi Academy on this computer, and the OpenJK-rpmod in it.
//!
//! An install is a GameData folder: the one with base/assets0.pk3 in it. On Windows it counts
//! when RPMod-Launcher is in its rpmod folder (or OpenJK-rpmod is already there). On Linux and
//! macOS, where RPMod-Launcher doesn't run, any Jedi Academy counts.

use crate::version::Version;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &[u8] = b"OpenJK-rpmod: v";

#[derive(Clone, Debug)]
pub struct Install {
    pub game_data: PathBuf,
    /// the OpenJK-rpmod in it, None when there's none
    pub version: Option<Version>,
    /// RPMod-Launcher in its rpmod folder (Windows), or an rpmod folder (Linux, macOS)
    pub has_rpmod: bool,
}

impl Install {
    pub fn read(game_data: &Path) -> Install {
        Install {
            game_data: game_data.to_path_buf(),
            version: installed_version(game_data),
            has_rpmod: has_rpmod(game_data),
        }
    }
}

/// the release download for this computer
pub fn asset_name() -> String {
    if cfg!(windows) {
        "OpenJK-windows-x86.zip".into()
    } else if cfg!(target_os = "macos") {
        format!("OpenJK-macos-{}.tar.gz", mac_arch())
    } else {
        "OpenJK-linux-x86.tar.gz".into()
    }
}

/// arm64 on Apple silicon, x86_64 on Intel, whichever this program was built for
#[cfg(target_os = "macos")]
pub fn mac_arch() -> &'static str {
    let arm = std::process::Command::new("sysctl")
        .args(["-n", "hw.optional.arm64"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(cfg!(target_arch = "aarch64"));
    if arm { "arm64" } else { "x86_64" }
}

#[cfg(not(target_os = "macos"))]
pub fn mac_arch() -> &'static str {
    "x86_64"
}

/// the multiplayer engine OpenJK-rpmod puts in GameData
pub fn engine_path(game_data: &Path) -> PathBuf {
    if cfg!(windows) {
        game_data.join("openjk.x86.exe")
    } else if cfg!(target_os = "macos") {
        let arch = mac_arch();
        game_data.join(format!("openjk.{arch}.app/Contents/MacOS/openjk.{arch}"))
    } else {
        game_data.join("openjk.i386")
    }
}

/// the version the engine was built as: it carries "OpenJK-rpmod: v0.7.3" in it
pub fn installed_version(game_data: &Path) -> Option<Version> {
    let data = fs::read(engine_path(game_data)).ok()?;
    let at = data.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len() - 1;
    let tag: String = data[at..]
        .iter()
        .take(64)
        .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        .map(|&b| b as char)
        .collect();
    Version::parse(&tag)
}

pub fn is_jka(dir: &Path) -> bool {
    let base = dir.join("base");
    base.join("assets0.pk3").is_file() || base.join("Assets0.pk3").is_file()
}

fn has_rpmod(game_data: &Path) -> bool {
    if cfg!(windows) {
        return game_data.join("rpmod").join("RPMod-Launcher.exe").is_file();
    }
    if game_data.join("rpmod").is_dir() {
        return true;
    }
    // OpenJK keeps a mod's files in the home folder too
    let home_rpmod = if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Application Support/OpenJK/rpmod"))
    } else {
        dirs::data_dir().map(|d| d.join("openjk/rpmod"))
    };
    home_rpmod.is_some_and(|p| p.is_dir())
}

/// what counts as an install of the game, for the list
fn counts(install: &Install) -> bool {
    if cfg!(windows) {
        install.has_rpmod || install.version.is_some()
    } else {
        true
    }
}

/// every install found, those with OpenJK-rpmod in them first
pub fn find() -> Vec<Install> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();

    for dir in candidates() {
        for game_data in [dir.join("GameData"), dir.clone()] {
            if !is_jka(&game_data) {
                continue;
            }
            let key = fs::canonicalize(&game_data)
                .unwrap_or_else(|_| game_data.clone())
                .to_string_lossy()
                .to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let install = Install::read(&game_data);
            if counts(&install) {
                found.push(install);
            }
        }
    }
    found.sort_by_key(|i| (i.version.is_none(), !i.has_rpmod));
    found
}

/// the folders under `dir`, `depth` deep, hidden and system ones left out
fn subdirs(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        if matches!(
            name.to_lowercase().as_str(),
            "windows" | "system volume information" | "node_modules" | "appdata" | "library" | "proc" | "sys" | "dev"
        ) {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let path = entry.path();
            out.push(path.clone());
            subdirs(&path, depth - 1, out);
        }
    }
}

/// Steam's libraries: its own folder and those listed in libraryfolders.vdf
fn steam_libraries(steam: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam.to_path_buf()];
    if let Ok(vdf) = fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // 	"path"		"D:\\SteamLibrary"
            if parts.len() >= 4 && parts[1].eq_ignore_ascii_case("path") {
                libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    libs
}

fn steam_game(libs: Vec<PathBuf>, out: &mut Vec<PathBuf>) {
    for lib in libs {
        let game = lib.join("steamapps").join("common").join("Jedi Academy");
        out.push(game.clone());
        // macOS: the game's folders inside its app
        out.push(game.join("Jedi Academy.app").join("Contents"));
    }
}

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut out = Vec::new();

    // Steam
    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
        if let Ok(path) = key.get_value::<String, _>("SteamPath") {
            steam_game(steam_libraries(Path::new(&path)), &mut out);
        }
    }
    // GOG
    for sub in ["SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1428935726", "SOFTWARE\\GOG.com\\Games\\1428935726"] {
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) {
            if let Ok(path) = key.get_value::<String, _>("path") {
                out.push(PathBuf::from(path));
            }
        }
    }
    // the CD version, and wherever else: each drive, two folders deep
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if !root.exists() {
            continue;
        }
        out.push(root.join("JKA"));
        for pf in ["Program Files (x86)", "Program Files"] {
            out.push(root.join(pf).join("LucasArts").join("Star Wars Jedi Knight Jedi Academy"));
            out.push(root.join(pf).join("Steam").join("steamapps").join("common").join("Jedi Academy"));
        }
        subdirs(&root, 2, &mut out);
    }
    out
}

#[cfg(target_os = "macos")]
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let steam = home.join("Library/Application Support/Steam");
        steam_game(steam_libraries(&steam), &mut out);
        out.push(home.join("JKA"));
        subdirs(&home, 3, &mut out);
    }
    out.push(PathBuf::from("/Applications"));
    subdirs(Path::new("/Applications"), 2, &mut out);
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for steam in [
            ".steam/steam",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "snap/steam/common/.local/share/Steam",
        ] {
            steam_game(steam_libraries(&home.join(steam)), &mut out);
        }
        out.push(home.join("JKA"));
        subdirs(&home, 3, &mut out);
    }
    for dir in ["/opt", "/usr/local/games", "/usr/games"] {
        out.push(PathBuf::from(dir));
        subdirs(Path::new(dir), 2, &mut out);
    }
    out
}
