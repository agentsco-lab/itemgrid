//! Updating item on the phone: built from its tree as its Debian package
//! (tools/package-deb.sh), installed with apt, and taken into use by a reboot - restarting the
//! vendor hwcomposer in place resets the Duo about one time in three
//! (item-tracker #119), a reboot never did.

use std::path::{Path, PathBuf};
use std::process::Command;


/// Where an update is, for whoever shows it.
#[derive(Debug, Clone)]
pub enum Step {
    Building,
    Installing,
    Boot(crate::phone::Boot),
}

impl Step {
    pub fn words(&self) -> &'static str {
        match self {
            Step::Building => "building item",
            Step::Installing => "installing it on the phone",
            Step::Boot(b) => b.words(),
        }
    }
}

/// item's tree: CRADLE_ITEM, else ~/.config/cradle/item (a path in it),
/// else ~/Desktop/projects/item/item/compositor if it is there.
pub fn item_tree() -> Result<PathBuf, String> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let from_env = std::env::var_os("CRADLE_ITEM").map(PathBuf::from);
    let from_file = std::fs::read_to_string(home.join(".config/cradle/item")).ok().map(|s| PathBuf::from(s.trim()));
    let guess = home.join("Desktop/projects/item/item/compositor");
    let tree = from_env.or(from_file).unwrap_or(guess);
    if tree.join("tools/install-phone.sh").exists() {
        Ok(tree)
    } else {
        Err(format!("item's tree not found at {} - set CRADLE_ITEM or write its path in ~/.config/cradle/item", tree.display()))
    }
}

fn cargo() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let local = home.join(".cargo/bin/cargo");
    if local.exists() { local } else { PathBuf::from("cargo") }
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed ({status})")) }
}

/// Builds item's package (unless `build` is false: then the newest built),
/// installs it on the phone at `host` with apt, reboots it and waits until
/// item runs again; `step` hears each stage.
pub fn update(host: &str, build: bool, step: &mut dyn FnMut(Step)) -> Result<(), String> {
    let _ = crate::phone::keep_awake(host, true);
    let tree = item_tree()?;
    if build {
        step(Step::Building);
        let mut c = Command::new("sh");
        c.current_dir(&tree).arg("tools/package-deb.sh").env("CARGO", cargo());
        run(&mut c, "the build")?;
    }
    let deb = newest_deb(&tree)?;
    step(Step::Installing);
    let ssh_opts = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR", "-q"];
    run(Command::new("scp").args(ssh_opts).arg(&deb).arg(format!("root@{host}:/var/tmp/item.deb")), "copying the package")?;
    // A changed conffile (the composer drop-in) takes the package's version:
    // there is no one to ask.
    crate::phone::run(host, "apt-get install -y -o Dpkg::Options::=--force-confnew /var/tmp/item.deb >/var/tmp/item-install.log 2>&1; rc=$?; rm -f /var/tmp/item.deb; sync; [ $rc = 0 ] || tail -5 /var/tmp/item-install.log; exit $rc\n")
        .map_err(|e| format!("installing the package: {e}"))?;
    crate::phone::reboot(host, &mut |b| step(Step::Boot(b)))
}

/// The newest package tools/package-deb.sh made.
fn newest_deb(tree: &Path) -> Result<PathBuf, String> {
    let dir = tree.join("target/deb");
    std::fs::read_dir(&dir)
        .map_err(|_| format!("no package built in {}", dir.display()))?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("item_") && e.path().extension().is_some_and(|x| x == "deb"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
        .ok_or_else(|| format!("no package built in {}", dir.display()))
}
