//! Hythe was Cradle: what Cradle kept here moved to Hythe's places - each
//! thing in them Hythe has not (installing put duo-motion in Hythe's own
//! before Hythe first ran), and Cradle's left empty taken away.

use std::path::PathBuf;

/// Cradle's places and Hythe's, under the home directory.
const PLACES: &[(&str, &str)] = &[
    (".config/cradle", ".config/hythe"),
    (".local/share/cradle", ".local/share/hythe"),
    (".local/state/cradle", ".local/state/hythe"),
    (".cache/cradle", ".cache/hythe"),
    ("cradle-backups", "hythe-backups"),
    ("cradle-shots", "hythe-shots"),
    ("cradle-logs", "hythe-logs"),
];

/// Moves them (a rename: the stock images and backups are not copied).
pub fn from_cradle() {
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
                eprintln!("hythe: {} not moved to {}: {e}", old.display(), new.display());
            }
            continue;
        }
        for entry in std::fs::read_dir(&old).into_iter().flatten().flatten() {
            let to = new.join(entry.file_name());
            if !to.exists() {
                if let Err(e) = std::fs::rename(entry.path(), &to) {
                    eprintln!("hythe: {} not moved to {}: {e}", entry.path().display(), to.display());
                }
            }
        }
        // Only if empty: what both had stays in Cradle's, untouched.
        let _ = std::fs::remove_dir(&old);
    }
}
