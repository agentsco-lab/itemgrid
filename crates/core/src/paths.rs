//! Where item/grid keeps things on this computer. On Linux the XDG places
//! under the home directory, as ever (`~/.local/share/itemgrid`,
//! `~/.config/itemgrid`, ...); on Windows under `%LOCALAPPDATA%\itemgrid`,
//! one folder each - the installer there keeps the same layout, so a
//! release folder is the same on both.

use std::path::PathBuf;

/// The user's home directory: HOME, or USERPROFILE on Windows.
pub fn home() -> PathBuf {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

#[cfg(windows)]
fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData/Local")).join("itemgrid")
}

/// Data that is kept: links, the release images, TWRP, stock kernels.
pub fn data() -> PathBuf {
    #[cfg(unix)]
    {
        home().join(".local/share/itemgrid")
    }
    #[cfg(windows)]
    {
        local_app_data().join("data")
    }
}

/// Settings.
pub fn config() -> PathBuf {
    #[cfg(unix)]
    {
        home().join(".config/itemgrid")
    }
    #[cfg(windows)]
    {
        local_app_data().join("config")
    }
}

/// What can be made again: downloaded packages, hashes worked out.
pub fn cache() -> PathBuf {
    #[cfg(unix)]
    {
        home().join(".cache/itemgrid")
    }
    #[cfg(windows)]
    {
        local_app_data().join("cache")
    }
}

/// State: the flashes' and RAM boots' record, when no port tree is here.
pub fn state() -> PathBuf {
    #[cfg(unix)]
    {
        home().join(".local/state/itemgrid")
    }
    #[cfg(windows)]
    {
        local_app_data().join("state")
    }
}

/// For this session only: the running job's note, ssh's shared connection.
pub fn runtime() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The backups: ~/itemgrid-backups (the owner's only - they hold keys).
pub fn backups() -> PathBuf {
    home().join("itemgrid-backups")
}

/// ~/.ssh.
pub fn ssh() -> PathBuf {
    home().join(".ssh")
}
