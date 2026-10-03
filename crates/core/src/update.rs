//! Updating item on the phone: built from its tree, installed with its own
//! tools/install-phone.sh, and taken into use by a reboot - restarting the
//! vendor hwcomposer in place resets the Duo about one time in three
//! (item-tracker #119), a reboot never did.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The packages install-phone.sh takes from the build.
const PACKAGES: &[&str] = &["item-compositor", "item-face", "pen-split"];
const TARGET: &str = "aarch64-unknown-linux-gnu";

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

/// Builds item (unless `build` is false), installs it on the phone at `host`,
/// reboots it and waits until item runs again; `step` hears each stage.
pub fn update(host: &str, build: bool, step: &mut dyn FnMut(Step)) -> Result<(), String> {
    let tree = item_tree()?;
    if build {
        step(Step::Building);
        let mut c = Command::new(cargo());
        c.current_dir(&tree).args(["build", "--release", "--target", TARGET]);
        for p in PACKAGES {
            c.args(["-p", p]);
        }
        run(&mut c, "the build")?;
    }
    step(Step::Installing);
    run(Command::new("sh").current_dir(&tree).arg(Path::new("tools/install-phone.sh")).arg(format!("root@{host}")), "the install")?;
    crate::phone::reboot(host, &mut |b| step(Step::Boot(b)))
}

