//! A job told as a journey: its stages in plain words, what each means for
//! the phone, how long each takes about - matched against the steps the
//! core says, so the window can show where a job is and what comes next.

/// One stage of a job.
pub struct Stage {
    pub title: &'static str,
    /// What happens, for someone watching.
    pub explain: &'static str,
    /// How the phone looks meanwhile (shown on the Duo drawn).
    pub phone: &'static str,
    /// Steps that begin it: a step starting with one of these (one starting
    /// with '=' must be the whole step).
    pub starts: &'static [&'static str],
    /// About how long it takes, seconds.
    pub secs: f64,
}

const fn st(title: &'static str, explain: &'static str, phone: &'static str, starts: &'static [&'static str], secs: f64) -> Stage {
    Stage { title, explain, phone, starts, secs }
}

const BACK_TO_LINUX: Stage = st(
    "Back to Linux",
    "Enter the PIN on the lock screen.",
    "Restarting",
    &["leaving the recovery", "Linux is back", "starting the port"],
    40.0,
);

/// A job's stages: as few as tell it (the owner's, 2026-10-30: fewer and
/// plainer), each a few words the window's board shows whole; the steps of
/// a stage gathered under it, their times summed.
pub fn stages(job: &str) -> Vec<Stage> {
    match job {
        "update" => vec![
            st("Building item", "On this computer.", "Linux", &["building item"], 90.0),
            st("Installing", "Copied to the phone and installed.", "Linux", &["installing it on the phone"], 25.0),
            st("Restarting", "The screens go dark for a moment.", "Restarting", &["rebooting the phone", "the phone went down", "the phone is back"], 80.0),
        ],
        "reboot" => vec![st("Restarting", "The screens go dark for a moment.", "Restarting", &[""], 80.0)],
        "ramboot" => vec![
            st("Getting ready", "Nothing is written to the phone.", "Restarting", &[""], 23.0),
            st("Starting the image", "A restart brings the old system back.", "Starting", &["starting it from memory", "waiting for"], 52.0),
        ],
        "recovery-exit" => vec![BACK_TO_LINUX],
        "full-backup" => vec![
            st("Getting ready", "Nothing on the phone changes.", "Restarting", &[""], 48.0),
            st("Copying everything", "About 90 GB, mostly empty, packed and checked.", "Recovery", &["mounting userdata", "taking rootfs.img", "hashing rootfs.img", "taking the Android container", "taking the rest of userdata"], 1220.0),
            BACK_TO_LINUX,
        ],
        "install-stock" => vec![
            // (Times as measured on the owner's Duo, 2026-10-30: 223 s in
            // all, the image sent as its zstd.)
            st("Getting ready", "Nothing is erased yet.", "Bootloader", &[""], 50.0),
            st("Writing item", "Erased, written, every byte checked.", "Recovery", &["making userdata", "erasing the phone's data", "writing item", "putting "], 70.0),
            st("First start", "item grows to fill the phone.", "Restarting", &["starting item from the computer", "writing boot", "restarting from the phone"], 105.0),
        ],
        "android-clean" => vec![
            st("Erasing item", "Linux and everything in it.", "Recovery", &[""], 480.0),
            st("Starting Android", "Android sets itself up.", "Restarting", &["the stock boot", "clearing the bootloader's note", "starting Android"], 45.0),
        ],
        "android-go" => vec![
            st("Backing up", "Everything copied here, so you can come back.", "Linux", &[""], 1250.0),
            // (Not "restarting into the bootloader": the backup says it too,
            // and this stage is in reach of it.)
            st("Erasing", "Tested first: if it fails, nothing is erased.", "Recovery", &["=into TWRP", "trying the way back", "zeroing metadata", "zeroing userdata"], 555.0),
            st("Starting Android", "Its first start sets itself up.", "Android", &["waiting for Android"], 70.0),
        ],
        "android-start" => vec![st("Starting Android", "From memory; nothing is written.", "Android", &[""], 60.0)],
        "android-back" => vec![
            st("Getting ready", "Android's data goes now.", "Restarting", &[""], 62.0),
            st("Putting Linux back", "Each part checked as it goes.", "Recovery", &["  image:", "checking every byte", "putting back"], 2060.0),
            st("First start", "Enter the PIN on the lock screen.", "Restarting", &["clearing the bootloader's note", "starting the port"], 120.0),
        ],
        "android-trial" => vec![st("Testing the way back", "512 MB sent and checked.", "Recovery", &[""], 30.0)],
        "restore" => vec![st("Restoring the boot chain", "Tried from memory, then written and read back.", "Restarting", &[""], 180.0)],
        "stock-download" => vec![
            st("Downloading", "Microsoft's package for this Duo, 2.6 GB. A break resumes.", "Linux", &[""], 600.0),
            st("Checking", "Against Microsoft's own hashes.", "Linux", &["taking boot out"], 8.0),
        ],
        "install" | "install-keep" | "install-full" => {
            let (ready, secs) = match job {
                "install-keep" => ("Your files copied here first.", 105.0),
                "install-full" => ("The whole system copied here first.", 1265.0),
                _ => ("Nothing is erased yet.", 20.0),
            };
            vec![
                st("Getting ready", ready, "Linux", &[""], secs + 65.0),
                st("Writing item", "Erased, written, every byte checked.", "Recovery", &["erasing the phone's data", "writing item"], 170.0),
                st("First start", "item grows to fill the phone.", "Starting", &["clearing the bootloader's note", "starting item:"], 180.0),
            ]
        }
        // backup and anything new: one stage.
        _ => vec![st("Backing up", "Your home and settings. The phone keeps working.", "Linux", &[""], 60.0)],
    }
}

/// The job in a few words, as a title.
pub fn title(job: &str) -> &'static str {
    match job {
        "update" => "Updating item",
        "reboot" => "Restarting the phone",
        "ramboot" => "Trying an image from RAM",
        "recovery-exit" => "Back to Linux",
        "full-backup" => "Backing up the whole system",
        "android-clean" => "Back to Android",
        "install-stock" => "Installing item",
        "android-go" => "Returning to Android",
        "android-start" => "Starting Android",
        "android-back" => "Back to the full backup",
        "android-trial" => "Testing the way back",
        "stock-download" => "Getting Android from Microsoft",
        "install" | "install-keep" | "install-full" => "Erase and install item",
        "restore" => "Restoring a boot chain",
        _ => "Backing up",
    }
}

/// What to do once it is done.
pub fn after(job: &str) -> &'static str {
    match job {
        "android-go" | "android-start" => "Android is on the phone. To come back to Linux, use Back to Linux here. Don't restart Android plainly: hold Volume Down + Power from off to reach the bootloader instead.",
        "full-backup" => "Everything is on this computer. Enter the PIN on the phone.",
        "install" => "item is installed afresh. Unlock with 1234, then choose your own PIN in Settings.",
        "install-keep" => "item is installed afresh with your files, Wi-Fi networks and PIN. Unlock as before.",
        "install-full" => "item is installed afresh. Unlock with 1234, then choose your own PIN. Your old system is on this computer: Back to Linux brings it back exactly.",
        "backup" => "The backup is on this computer.",
        "android-trial" => "The way back works.",
        "stock-download" => "Microsoft's Android for this Duo is on this computer: item/grid uses it to return to Android, and to repair it.",
        _ => "Enter the PIN on the phone when the lock screen appears.",
    }
}

/// The stage the steps have reached: a step can move it on by one or two
/// stages, never further (a step a later stage also knows - "into the
/// bootloader" inside a backup - does not jump ahead).
pub fn locate(stages: &[Stage], lines: &[String]) -> usize {
    let mut at = 0;
    for line in lines {
        let hit = |s: &Stage| {
            s.starts.iter().any(|p| match p.strip_prefix('=') {
                Some(exact) => line == exact,
                None => line.starts_with(p),
            })
        };
        if let Some(i) = (at + 1..stages.len().min(at + 3)).find(|&i| hit(&stages[i])) {
            at = i;
        }
    }
    at
}

/// How far a step that counts says it is ("  downloaded: 812 of 2509 MB").
pub fn counted(line: &str) -> Option<f64> {
    let rest = line.trim_start().split_once(": ")?.1;
    let (done, rest) = rest.split_once(" of ")?;
    let whole = rest.split_whitespace().next()?;
    let (d, w): (f64, f64) = (done.trim().parse().ok()?, whole.parse().ok()?);
    (w > 0.0).then(|| (d / w).clamp(0.0, 1.0))
}

/// How far into a stage after `secs` there: quick at first, never quite
/// full until the next stage begins.
pub fn within(stage: &Stage, secs: f64) -> f64 {
    (1.0 - (-secs / stage.secs.max(1.0)).exp()).min(0.95)
}

/// About how long is left, seconds.
pub fn left(stages: &[Stage], at: usize, secs_in: f64) -> f64 {
    let rest: f64 = stages.iter().skip(at + 1).map(|s| s.secs).sum();
    rest + (stages[at].secs - secs_in).max(stages[at].secs * 0.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An install keeping files, as it said itself on the owner's Duo
    /// (2026-10-29): every stage reached, the last at the end.
    #[test]
    fn an_install_goes_through() {
        let s = stages("install-keep");
        let log = [
            "checking the image here",
            "backing up your files and settings",
            "noting the installed packages",
            "copying your home folder and settings",
            "checking the copy",
            "copying the fingerprints",
            "Home and settings backed up: 319 MB",
            "starting the recovery",
            "checking the phone and the image to start",
            "the image is sound",
            "setting the safety catch: a failed start stops in the bootloader",
            "restarting into the bootloader",
            "in the bootloader: checking it is the same phone, healthy",
            "starting it from memory, nothing written (try 1 of 2)",
            "waiting for the recovery",
            "the recovery is up",
            "the phone is in the recovery",
            "testing the cable: 512 MB sent and checked",
            "erasing the phone's data partition",
            "writing item 0.3.1",
            "  512 MB of 1.4 GB sent",
            "checking every byte on the phone",
            "letting this computer in (its ssh key)",
            "putting your files back (390 MB)",
            "putting the fingerprints back",
            "clearing the bootloader's note",
            "starting item: the first start takes a minute longer",
        ];
        let mut lines = Vec::new();
        let mut seen = Vec::new();
        for l in log {
            lines.push(l.to_string());
            seen.push(locate(&s, &lines));
        }
        assert_eq!(*seen.last().unwrap(), s.len() - 1, "{seen:?}");
        for i in 0..s.len() {
            assert!(seen.contains(&i), "stage {i} ({}) never shown: {seen:?}", s[i].title);
        }
    }

    /// An install from stock Android, as it said itself on the owner's Duo
    /// (2026-10-30): "putting item on" was skipped then, its steps renamed.
    #[test]
    fn an_install_from_stock_goes_through() {
        let s = stages("install-stock");
        let log = [
            "checking the image here",
            "into the bootloader",
            "starting the recovery",
            "in the bootloader: checking it is the same phone, healthy",
            "starting it from memory, nothing written (try 1 of 2)",
            "the recovery is up: the RAM boot is confirmed",
            "erasing the phone's data partition",
            "writing item 0.3.1",
            "  512 MB of 1.4 GB sent",
            "checking every byte on the phone",
            "clearing the bootloader's note",
            "into the bootloader",
            "starting item from the computer - its first start grows it to fill the phone",
            "Linux is up: the RAM boot is confirmed",
            "writing boot_b",
            "restarting from the phone's own boot",
        ];
        let mut lines = Vec::new();
        let mut seen = Vec::new();
        for l in log {
            lines.push(l.to_string());
            seen.push(locate(&s, &lines));
        }
        for i in 0..s.len() {
            assert!(seen.contains(&i), "stage {i} ({}) never shown: {seen:?}", s[i].title);
        }
    }

    #[test]
    fn a_backup_inside_does_not_jump_ahead() {
        let s = stages("android-go");
        let lines: Vec<String> = ["taking the whole system's backup first", "into TWRP, from RAM", "restarting into the bootloader", "starting it from memory, nothing written (try 1 of 2)"].iter().map(|l| l.to_string()).collect();
        assert_eq!(locate(&s, &lines), 0);
        let mut more = lines.clone();
        more.extend(["into TWRP", "checking the phone and the image to start", "restarting into the bootloader", "trying the way back (512 MB onto the phone, checked)"].iter().map(|l| l.to_string()));
        assert_eq!(locate(&s, &more), 1);
        more.push("zeroing metadata (/dev/block/sda3) whole".into());
        more.push("restarting into the bootloader".into());
        assert_eq!(locate(&s, &more), 1);
        more.push("waiting for Android".into());
        assert_eq!(locate(&s, &more), 2);
    }
}
