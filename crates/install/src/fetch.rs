//! What the installer brings here before it touches the phone: adb and
//! fastboot (Google's platform-tools, on Windows), and the newest system
//! image release from item's GitHub releases - every file checked against
//! the release's SHA256SUMS, the big one resumed if the download stopped.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Google's platform-tools for Windows: adb, fastboot and their DLLs.
const PLATFORM_TOOLS: &str = "https://dl.google.com/android/repository/platform-tools-latest-windows.zip";
/// item's releases.
const RELEASES: &str = "https://api.github.com/repos/agentsco-lab/item/releases";
/// A system image release's tag begins so.
const IMAGE_TAG: &str = "item-duo1-";

pub type Say<'a> = &'a mut dyn FnMut(String);

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(None).user_agent("itemgrid-install").build().new_agent()
}

/// adb and fastboot here: on Windows, platform-tools unpacked into
/// item/grid's tools folder (and ITEMGRID_TOOLS set to it); elsewhere the
/// system's own. Returns where they are, or why not.
pub fn tools(say: Say) -> Result<(), String> {
    if cfg!(windows) {
        let dir = itemgrid_core::paths::data().join("tools");
        let adb = dir.join("adb.exe");
        if !adb.exists() {
            say("getting adb and fastboot (Google's platform-tools)".into());
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let zip = dir.join("platform-tools.zip");
            download(PLATFORM_TOOLS, &zip, &mut |_, _| {})?;
            unpack_flat(&zip, &dir)?;
            let _ = std::fs::remove_file(&zip);
        }
        // Found by programs::find from now on.
        std::env::set_var("ITEMGRID_TOOLS", &dir);
    }
    for t in ["adb", "fastboot"] {
        if !itemgrid_core::programs::present(t) {
            return Err(format!("{t} was not found{}", if cfg!(windows) { "" } else { ": install android-tools (adb, fastboot)" }));
        }
    }
    Ok(())
}

/// A zip's files unpacked into `dir`, flat (platform-tools/adb.exe ->
/// adb.exe), directories skipped.
fn unpack_flat(zip: &Path, dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", zip.display()))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = Path::new(entry.name()).file_name().map(|n| n.to_owned()) else { continue };
        let mut out = std::fs::File::create(dir.join(name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A system image release as GitHub lists it.
#[derive(Debug, Clone)]
pub struct Listed {
    pub tag: String,
    /// Asset name -> (download url, size).
    pub assets: Vec<(String, String, u64)>,
}

/// The newest system image release on GitHub (the newest tag item-duo1-…
/// with a manifest among its files).
pub fn newest_release() -> Result<Listed, String> {
    let mut resp = agent().get(RELEASES).call().map_err(|e| format!("GitHub: {e}"))?;
    let list: serde_json::Value = resp.body_mut().read_json().map_err(|e| format!("GitHub's answer: {e}"))?;
    let mut found: Vec<Listed> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true && r["prerelease"] != true)
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?.to_owned();
            if !tag.starts_with(IMAGE_TAG) {
                return None;
            }
            let assets: Vec<(String, String, u64)> = r["assets"]
                .as_array()?
                .iter()
                .filter_map(|a| Some((a["name"].as_str()?.to_owned(), a["browser_download_url"].as_str()?.to_owned(), a["size"].as_u64().unwrap_or(0))))
                .collect();
            assets.iter().any(|(n, _, _)| n == "manifest.json").then_some(Listed { tag, assets })
        })
        .collect();
    found.sort_by(|a, b| b.tag.cmp(&a.tag));
    found.into_iter().next().ok_or_else(|| "no system image release was found on GitHub".to_owned())
}

/// The release's files here, in item/grid's releases folder, each checked
/// against SHA256SUMS (a file already here and right is not fetched again).
/// Returns the folder.
pub fn release(listed: &Listed, say: Say) -> Result<PathBuf, String> {
    let dir = itemgrid_core::paths::data().join("releases").join(&listed.tag);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let url_of = |name: &str| listed.assets.iter().find(|(n, _, _)| n == name).map(|(_, u, s)| (u.clone(), *s)).ok_or_else(|| format!("the release has no {name}"));
    // The checksums first, then everything they name.
    let (sums_url, _) = url_of("SHA256SUMS")?;
    download(&sums_url, &dir.join("SHA256SUMS"), &mut |_, _| {})?;
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let wanted: Vec<(String, String)> = sums
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?.to_owned(), w.next()?.trim_start_matches('*').to_owned()))
        })
        .collect();
    if wanted.is_empty() {
        return Err("the release's SHA256SUMS is empty".into());
    }
    for (sha, name) in &wanted {
        let path = dir.join(name);
        if path.exists() && sha_of(&path)? == *sha {
            continue;
        }
        let (url, size) = url_of(name)?;
        say(format!("getting {name} ({})", mb(size)));
        let mut last = String::new();
        download(&url, &path, &mut |done, whole| {
            if whole < (1 << 20) {
                return;
            }
            let line = format!("{name}: {} of {}", mb(done), mb(whole));
            if line != last {
                say(line.clone());
                last = line;
            }
        })?;
        say(format!("checking {name}"));
        if sha_of(&path)? != *sha {
            let _ = std::fs::remove_file(&path);
            return Err(format!("{name} arrived different from the release's checksum - run it again"));
        }
    }
    Ok(dir)
}

/// Bytes in words: "1163 MB", or "1 KB" below a megabyte.
fn mb(n: u64) -> String {
    if n >= (1 << 20) { format!("{} MB", n >> 20) } else { format!("{} KB", (n >> 10).max(1)) }
}

fn sha_of(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// `url` into `path`, going on from a `.part` left by a stopped download.
fn download(url: &str, path: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let part = PathBuf::from(format!("{}.part", path.display()));
    let have = part.metadata().map(|m| m.len()).unwrap_or(0);
    let mut req = agent().get(url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("{url}: {e}"))?;
    let resumed = resp.status() == 206;
    let rest: u64 = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let (mut done, whole) = if resumed { (have, have + rest) } else { (0, rest) };
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part).map_err(|e| e.to_string())?;
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut last = std::time::Instant::now();
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("downloading: {e} - run it again to go on from here"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed().as_millis() > 500 {
            progress(done, whole);
            last = std::time::Instant::now();
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    progress(done, whole);
    if whole > 0 && done != whole {
        return Err(format!("the download stopped at {} of {} MB - run it again to go on", done >> 20, whole >> 20));
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}
