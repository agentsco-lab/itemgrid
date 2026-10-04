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

const CHECKING: Stage = st(
    "Getting ready",
    "Cradle checks the image and the phone's health, and sets a safety catch: if anything goes wrong, the phone stops in its bootloader instead of trying again.",
    "Linux",
    &["checking the image", "image fine", "arming the parking brake", "into TWRP, from RAM"],
    8.0,
);
const TO_BOOTLOADER: Stage = st(
    "Restarting into the bootloader",
    "The phone restarts into its bootloader. The screens go dark for a few seconds - that is expected.",
    "Restarting",
    &["into the bootloader", "in fastboot", "re-arming the parking brake in fastboot"],
    15.0,
);
const TWRP_UP: Stage = st(
    "Starting the recovery",
    "TWRP, a small repair system, starts from the computer's memory - nothing is written to the phone. It has no touch: leave the phone as it is.",
    "TWRP",
    &["booting the image from RAM", "waiting for the recovery", "the recovery is up"],
    25.0,
);
const BACK_TO_LINUX: Stage = st(
    "Restarting into Linux",
    "The phone restarts into Linux. Enter the PIN when the lock screen appears.",
    "Restarting",
    &["leaving the recovery", "Linux is back", "starting the port"],
    40.0,
);

pub fn stages(job: &str) -> Vec<Stage> {
    match job {
        "update" => vec![
            st("Building item", "item is built from your tree on this computer.", "Linux", &["building item"], 90.0),
            st("Installing", "The new item is copied to the phone and installed.", "Linux", &["installing it on the phone"], 25.0),
            st("Restarting", "The phone restarts to run the new item. The screens go dark for a moment.", "Restarting", &["rebooting the phone", "the phone went down"], 60.0),
            st("Starting item", "Linux is up again; item is starting.", "Linux", &["the phone is back"], 20.0),
        ],
        "reboot" => vec![
            st("Restarting", "The phone restarts. The screens go dark for a moment.", "Restarting", &["rebooting the phone", "the phone went down"], 60.0),
            st("Starting item", "Linux is up again; item is starting.", "Linux", &["the phone is back"], 20.0),
        ],
        "ramboot" => vec![
            CHECKING,
            TO_BOOTLOADER,
            st("Starting the image from RAM", "The image is sent over USB and started once. Nothing is written to the phone; a restart brings the old system back.", "Starting", &["booting the image from RAM"], 12.0),
            st("Waiting for it to start", "The image is starting up on the phone.", "Starting", &["waiting for"], 40.0),
        ],
        "recovery-exit" => vec![BACK_TO_LINUX],
        "full-backup" => vec![
            CHECKING,
            TO_BOOTLOADER,
            TWRP_UP,
            st("Copying the system", "Linux's whole system - about 90 GB, mostly empty space - is compressed on the phone and copied to this computer.", "TWRP", &["mounting userdata", "taking rootfs.img"], 620.0),
            st("Checking the copy", "The phone and this computer each work out a fingerprint of the system; they must match exactly.", "TWRP", &["hashing rootfs.img"], 360.0),
            st("Copying the rest", "Android apps' data and the other files on the data partition are copied too.", "TWRP", &["taking the Android container", "taking the rest of userdata"], 240.0),
            BACK_TO_LINUX,
        ],
        "android-go" => vec![
            st("Backing up everything", "Everything on the phone is copied to this computer first - or a fresh copy is checked - so the way back is certain.", "TWRP", &["checking ", "taking the whole system's backup first"], 1250.0),
            st("Starting the recovery", "TWRP starts from RAM to do what Linux cannot do to itself.", "TWRP", &["=into TWRP"], 50.0),
            st("Testing the way back", "512 MB are sent to the phone and checked there. If this fails, nothing is erased.", "TWRP", &["trying the way back"], 30.0),
            st("Erasing Linux's data", "The data partition is cleared so Android can start fresh. Your backup holds everything that was there.", "TWRP", &["zeroing metadata", "zeroing userdata"], 460.0),
            TO_BOOTLOADER,
            st("Starting Android", "Stock Android starts from RAM. Its first start sets itself up and opens the welcome screens.", "Android", &["booting the image from RAM", "waiting for Android"], 70.0),
        ],
        "android-start" => vec![
            TO_BOOTLOADER,
            st("Starting Android", "Stock Android starts from RAM.", "Android", &["booting the image from RAM", "waiting for Android"], 45.0),
        ],
        "android-back" => vec![
            st("Getting ready", "The newest whole-system backup is chosen.", "Android", &["coming back from"], 2.0),
            TO_BOOTLOADER,
            TWRP_UP,
            st("Preparing the data partition", "The data partition is made ready for Linux again. Android's data goes now.", "TWRP", &["making userdata ext4"], 20.0),
            st("Putting Linux back", "The system goes back in 512 MB parts; each part is checked on the phone and sent again if it differs.", "TWRP", &["  image:"], 1500.0),
            st("Checking the system", "The whole system on the phone is checked against the backup's fingerprint.", "TWRP", &["checking the whole image"], 360.0),
            st("Putting the rest back", "Android apps' data and the other files go back.", "TWRP", &["putting back"], 200.0),
            st("Restarting into Linux", "The phone restarts into Linux - the first start takes a little longer. Enter the PIN when the lock screen appears.", "Restarting", &["clearing misc", "starting the port"], 120.0),
        ],
        "stock-download" => vec![
            st("Downloading Android from Microsoft", "Microsoft's own package for this Duo - about 2.6 GB - comes straight from Microsoft's servers, asked for by its serial number. A break is no loss: it goes on from where it stopped.", "Linux", &["downloading"], 600.0),
            st("Checking and preparing", "The package is checked, and the stock boot chain taken out of it against the package's own hashes - so a return to Android never depends on a backup.", "Linux", &["taking boot out"], 8.0),
        ],
        "install" | "install-keep" | "install-full" => {
            let backup = match job {
                "install-keep" => st("Keeping your files", "Your home folder, Wi-Fi networks, time zone and PIN are copied to this computer, to go into the new system.", "Linux", &["backing up your files", "backing up the device data"], 90.0),
                "install-full" => st("Backing up everything", "The whole system is copied to this computer first - or a fresh copy is used - so it can come back exactly.", "TWRP", &["the whole system was backed up", "taking the whole system's backup first", "backing up the device data"], 1250.0),
                _ => st("Getting ready", "Nothing on the phone is kept - it starts afresh. Its device data (radio calibration, IMEI) is not touched.", "Linux", &["nothing is kept", "backing up the device data"], 5.0),
            };
            vec![
                st("Checking the image", "The release image is checked here against its manifest before anything happens.", "Linux", &["checking the image"], 40.0),
                backup,
                st("Starting the recovery", "TWRP starts from RAM to do the work.", "TWRP", &["=into TWRP"], 50.0),
                st("Testing the way in", "512 MB are sent to the phone and checked there. If this fails, nothing is erased.", "TWRP", &["trying the way in"], 30.0),
                st("Putting the new system on", "The data partition is made anew and the new system written in checked parts - about 5 GB.", "TWRP", &["making userdata anew", "putting item-"], 240.0),
                st("Checking it on the phone", "The whole system on the phone is checked against the release; this computer's key goes in, and anything kept.", "TWRP", &["checking the whole image on the phone"], 80.0),
                st("First start", "The new system starts for the first time and grows to fill the phone - a little longer than usual.", "Starting", &["clearing misc", "starting the new system"], 180.0),
            ]
        }
        "android-trial" => vec![st("Testing the way back", "512 MB are sent to the phone and checked there.", "TWRP", &["trying the way back"], 30.0)],
        "restore" => vec![st("Restoring the boot chain", "A changed boot is tried from RAM first; then each partition is written and read back.", "Restarting", &[""], 180.0)],
        // backup and anything new: one stage.
        _ => vec![st("Backing up", "The boot chain, your home and settings are copied to this computer. The phone keeps working.", "Linux", &[""], 60.0)],
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
        "android-go" => "Returning to Android",
        "android-start" => "Starting Android",
        "android-back" => "Back to Linux",
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
        "stock-download" => "Microsoft's Android for this Duo is on this computer: Cradle uses it to return to Android, and to repair it.",
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

    #[test]
    fn a_backup_inside_does_not_jump_ahead() {
        let s = stages("android-go");
        let lines: Vec<String> = ["taking the whole system's backup first", "into TWRP, from RAM", "into the bootloader", "booting the image from RAM (attempt 1 of 2)"].iter().map(|l| l.to_string()).collect();
        assert_eq!(locate(&s, &lines), 0);
        let mut more = lines.clone();
        more.extend(["into TWRP", "checking the image and the phone", "into the bootloader", "trying the way back (512 MB onto the phone, checked)"].iter().map(|l| l.to_string()));
        assert_eq!(locate(&s, &more), 2);
        more.push("zeroing metadata (/dev/block/sda3) whole".into());
        more.push("into the bootloader".into());
        assert_eq!(locate(&s, &more), 4);
    }
}
