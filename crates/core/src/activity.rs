//! What Gridbay is doing with the phone right now, wherever it runs: a long
//! job (the command line's, or the window's) writes its steps to a small
//! file, so a window open meanwhile shows them - a backup going on, not a
//! phone that vanished. $XDG_RUNTIME_DIR/gridbay-activity.json.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A job's record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub pid: u32,
    /// The job's kind, for the window's stages: update, reboot, backup,
    /// full-backup, ramboot, recovery-exit, restore, android-go,
    /// android-start, android-back.
    pub job: String,
    pub started: u64,
    /// Its steps so far, as said.
    pub lines: Vec<String>,
    /// Once over: None while running; Some(None) done; Some(Some(why)) stopped.
    pub ended: Option<Option<String>>,
    pub ended_at: Option<u64>,
}

static CURRENT: Mutex<Option<Activity>> = Mutex::new(None);

fn path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("gridbay-activity.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write(a: &Activity) {
    if let Ok(text) = serde_json::to_string(a) {
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

/// A job begun in this process.
pub fn begin(job: &str) {
    let a = Activity { pid: std::process::id(), job: job.to_owned(), started: now(), lines: Vec::new(), ended: None, ended_at: None };
    write(&a);
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(a);
}

/// A step of this process's job.
pub fn line(words: &str) {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(a) = cur.as_mut() {
        a.lines.push(words.to_owned());
        // A long transfer's notes would grow it without end.
        let extra = a.lines.len().saturating_sub(400);
        a.lines.drain(..extra);
        write(a);
    }
}

/// This process's job over.
pub fn end(result: &Result<(), String>) {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut a) = cur.take() {
        a.ended = Some(result.as_ref().err().cloned());
        a.ended_at = Some(now());
        write(&a);
    }
}

/// The job another process is running, or ended within `recent` seconds.
pub fn elsewhere(recent: u64) -> Option<Activity> {
    let a: Activity = serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
    if a.pid == std::process::id() {
        return None;
    }
    match a.ended_at {
        Some(t) => (now().saturating_sub(t) < recent).then_some(a),
        // Still running only if its process is.
        None => std::path::Path::new(&format!("/proc/{}", a.pid)).exists().then_some(a),
    }
}

impl Activity {
    /// How it ended: None while running; Some(None) done; Some(Some(why))
    /// stopped. (Read from the file, "done" and "running" both come back as
    /// a null `ended`: the end time tells them apart.)
    pub fn outcome(&self) -> Option<Option<String>> {
        self.ended_at.map(|_| self.ended.clone().flatten())
    }

    pub fn seconds(&self) -> u64 {
        self.ended_at.unwrap_or_else(now).saturating_sub(self.started)
    }
}
