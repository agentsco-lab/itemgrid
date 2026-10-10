//! item/grid was Gridbay, Hythe and Cradle before that: what they kept here
//! moved to item/grid's places - each thing in them it has not (installing
//! put duo-motion in its own before it first ran), and theirs left empty
//! taken away.

/// The old names' places and item/grid's, under the home directory (the
/// newest name first).
const PLACES: &[(&str, &str)] = &[
    (".config/gridbay", ".config/itemgrid"),
    (".local/share/gridbay", ".local/share/itemgrid"),
    (".local/state/gridbay", ".local/state/itemgrid"),
    (".cache/gridbay", ".cache/itemgrid"),
    ("gridbay-backups", "itemgrid-backups"),
    ("gridbay-shots", "itemgrid-shots"),
    ("gridbay-logs", "itemgrid-logs"),
    (".config/hythe", ".config/itemgrid"),
    (".local/share/hythe", ".local/share/itemgrid"),
    (".local/state/hythe", ".local/state/itemgrid"),
    (".cache/hythe", ".cache/itemgrid"),
    ("hythe-backups", "itemgrid-backups"),
    ("hythe-shots", "itemgrid-shots"),
    ("hythe-logs", "itemgrid-logs"),
    (".config/cradle", ".config/itemgrid"),
    (".local/share/cradle", ".local/share/itemgrid"),
    (".local/state/cradle", ".local/state/itemgrid"),
    (".cache/cradle", ".cache/itemgrid"),
    ("cradle-backups", "itemgrid-backups"),
    ("cradle-shots", "itemgrid-shots"),
    ("cradle-logs", "itemgrid-logs"),
];

/// Moves them (a rename: the stock images and backups are not copied).
pub fn from_old_names() {
    let home = crate::paths::home();
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
                eprintln!("itemgrid: {} not moved to {}: {e}", old.display(), new.display());
            }
            continue;
        }
        for entry in std::fs::read_dir(&old).into_iter().flatten().flatten() {
            let to = new.join(entry.file_name());
            if !to.exists() {
                if let Err(e) = std::fs::rename(entry.path(), &to) {
                    eprintln!("itemgrid: {} not moved to {}: {e}", entry.path().display(), to.display());
                }
            }
        }
        // Only if empty: what both had stays in the old one's, untouched.
        let _ = std::fs::remove_dir(&old);
    }
}
