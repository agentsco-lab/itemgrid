//! The RAM boots' and flashes' state, shared with the port's
//! tools/flash-safely.sh (schema v2, by serial): one counter and one history
//! for both, so neither can go around the other's limit. On a computer with
//! the port's tree it is that tree's out/flash-state.json; elsewhere
//! ~/.local/state/cradle/flash-state.json. Read here only, for now.

use std::path::{Path, PathBuf};

/// RAM boots in a row not yet confirmed, before the gate closes.
pub const MAX_UNCONFIRMED: u64 = 2;

/// The port's tree: CRADLE_PORT, ~/.config/cradle/port, or the usual place.
pub fn port_tree() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let from_env = std::env::var_os("CRADLE_PORT").map(PathBuf::from);
    let from_file = std::fs::read_to_string(home.join(".config/cradle/port")).ok().map(|s| PathBuf::from(s.trim()));
    let tree = from_env.or(from_file).unwrap_or_else(|| home.join("Desktop/projects/surfaceduo/surfaceduo-droidian"));
    tree.join("tools/flash-safely.sh").exists().then_some(tree)
}

pub fn state_path() -> PathBuf {
    match port_tree() {
        Some(t) => t.join("out/flash-state.json"),
        None => Path::new(&std::env::var("HOME").unwrap_or_default()).join(".local/state/cradle/flash-state.json"),
    }
}

/// This phone's part of the state.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub unconfirmed: u64,
    pub confirmed_sha256: Option<String>,
    /// The last few events, newest last.
    pub history: Vec<(String, String)>,
}

impl Gate {
    pub fn open(&self) -> bool {
        self.unconfirmed < MAX_UNCONFIRMED
    }
}

pub fn gate(serial: &str) -> Gate {
    let Ok(text) = std::fs::read_to_string(state_path()) else { return Gate::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Gate::default() };
    let d = &v["devices"][serial];
    Gate {
        unconfirmed: d["consecutive_unconfirmed"].as_u64().unwrap_or(0),
        confirmed_sha256: d["confirmed_sha256"].as_str().map(str::to_owned),
        history: d["history"]
            .as_array()
            .map(|h| h.iter().map(|e| (e["ts"].as_str().unwrap_or_default().to_owned(), e["event"].as_str().unwrap_or_default().to_owned())).collect())
            .unwrap_or_default(),
    }
}
