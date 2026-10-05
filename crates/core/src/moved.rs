//! Gridbay was Hythe, and Cradle before that: what they kept here moved to
//! Gridbay's places - each thing in them Gridbay has not (installing put
//! duo-motion in Gridbay's own before it first ran), and theirs left empty
//! taken away.

use std::path::PathBuf;

/// The old names' places and Gridbay's, under the home directory (Hythe's
/// first: the newer).
const PLACES: &[(&str, &str)] = &[
    (".config/hythe", ".config/gridbay"),
    (".local/share/hythe", ".local/share/gridbay"),
    (".local/state/hythe", ".local/state/gridbay"),
    (".cache/hythe", ".cache/gridbay"),
    ("hythe-backups", "gridbay-backups"),
    ("hythe-shots", "gridbay-shots"),
    ("hythe-logs", "gridbay-logs"),
    (".config/cradle", ".config/gridbay"),
    (".local/share/cradle", ".local/share/gridbay"),
    (".local/state/cradle", ".local/state/gridbay"),
    (".cache/cradle", ".cache/gridbay"),
    ("cradle-backups", "gridbay-backups"),
    ("cradle-shots", "gridbay-shots"),
    ("cradle-logs", "gridbay-logs"),
];

/// Moves them (a rename: the stock images and backups are not copied).
pub fn from_old_names() {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    for (old, new) in PLACES {
        let (old, new) = (home.join(old), home.join(new));
        if !old.exists() {
            continue;
        }
        if !new.exists() {
            if let Some(parent) = new.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::rename(&old, &new) {
                eprintln!("gridbay: {} not moved to {}: {e}", old.display(), new.display());
            }
            continue;
        }
        for entry in std::fs::read_dir(&old).into_iter().flatten().flatten() {
            let to = new.join(entry.file_name());
            if !to.exists() {
                if let Err(e) = std::fs::rename(entry.path(), &to) {
                    eprintln!("gridbay: {} not moved to {}: {e}", entry.path().display(), to.display());
                }
            }
        }
        // Only if empty: what both had stays in the old one's, untouched.
        let _ = std::fs::remove_dir(&old);
    }
}
