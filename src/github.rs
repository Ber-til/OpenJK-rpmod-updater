//! OpenJK-rpmod's releases on GitHub: which there are, their notes, and their downloads.

use crate::version::Version;
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub const REPO: &str = "Ber-til/OpenJK-rpmod";
pub const RELEASES_PAGE: &str = "https://github.com/Ber-til/OpenJK-rpmod/releases/latest";
const USER_AGENT: &str = concat!("OpenJK-rpmod-updater/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize, Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

#[derive(Deserialize, Clone, Debug)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// the download for this computer
    pub fn asset(&self) -> Option<&Asset> {
        let name = crate::installs::asset_name();
        self.assets.iter().find(|a| a.name == name)
    }
}

fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(15)))
        .build()
        .new_agent()
}

/// GitHub escapes these in some release notes
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// the releases, newest first; drafts, pre-releases and tags that aren't versions left out
pub fn releases() -> Result<Vec<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let mut response = agent(Some(Duration::from_secs(20)))
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => format!("GitHub answered {code}"),
            other => format!("GitHub couldn't be reached ({other})"),
        })?;
    let text = response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("GitHub's answer couldn't be read ({e})"))?;
    let raw: Vec<RawRelease> = serde_json::from_str(&text).map_err(|e| format!("GitHub's answer couldn't be read ({e})"))?;

    let mut releases: Vec<Release> = raw
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(&r.tag_name)?;
            Some(Release {
                version,
                notes: unescape(r.body.as_deref().unwrap_or("")),
                assets: r.assets,
            })
        })
        .collect();
    releases.sort_by(|a, b| b.version.nums.cmp(&a.version.nums));
    Ok(releases)
}

/// the notes of every release after `from`, newest first; all of the newest one's when `from` is None
pub fn notes_since(releases: &[Release], from: Option<&Version>) -> String {
    let mut notes = String::new();
    for release in releases {
        if let Some(from) = from {
            if !from.is_older_than(&release.version) {
                continue;
            }
        } else if !notes.is_empty() {
            break;
        }
        let body = release.notes.trim();
        if !notes.is_empty() {
            notes.push_str("\n\n");
        }
        if body.starts_with('#') {
            notes.push_str(body);
        } else {
            notes.push_str(&format!("## {}\n\n{}", release.version, body));
        }
    }
    notes
}

/// the download into `to`, `progress` told the bytes so far
pub fn download(asset: &Asset, to: &Path, mut progress: impl FnMut(u64)) -> Result<(), String> {
    let mut response = agent(None)
        .get(&asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("The download couldn't start ({e})"))?;
    let mut reader = response.body_mut().with_config().limit(1024 * 1024 * 1024).reader();
    let mut file = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("The download stopped ({e})"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", to.display()))?;
        done += n as u64;
        progress(done);
    }
    file.flush().map_err(|e| format!("{}: {e}", to.display()))?;
    if asset.size != 0 && done != asset.size {
        return Err(format!("The download was cut short ({done} of {} bytes)", asset.size));
    }
    Ok(())
}
