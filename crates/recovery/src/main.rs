//! item-recovery: what item/grid boots on the Duo from RAM (fastboot boot,
//! nothing flashed) to put a system on it or take one off - the port's
//! kernel with an initramfs of this one program, busybox for the shell and
//! mke2fs. As PID 1 it mounts what a shell needs, names the partitions as
//! Android does (/dev/block/.../by-name), raises the USB gadget with an
//! adb function under the phone's own serial and runs an adb daemon of its
//! own: a shell with exit codes, push and pull, a stream into a command,
//! reboot. Nothing else: no network, no touch, no Android.
//!
//! As `reboot`, `poweroff` or `halt` (busybox's would signal PID 1): the
//! filesystems synced, then that. As `truncate` and `mountpoint`: those,
//! which the port's busybox has not.

mod adb;
mod init;
mod log;
mod tools;

fn main() {
    let argv0 = std::env::args().next().unwrap_or_default();
    let name = argv0.rsplit('/').next().unwrap_or_default().to_owned();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match name.as_str() {
        "reboot" => init::reboot(args.iter().find(|a| !a.starts_with('-')).map(String::as_str)),
        "poweroff" | "halt" => init::power_off(),
        // What item/grid uses and the port's busybox lacks.
        "truncate" => std::process::exit(tools::truncate(&args)),
        "mountpoint" => std::process::exit(tools::mountpoint(&args)),
        _ => match args.first().map(String::as_str) {
            // The daemon, as init's child (init reaps orphans; the daemon
            // waits for its own children).
            Some("adbd") => adb::run(),
            _ if std::process::id() == 1 => init::run(),
            _ => {
                eprintln!("item-recovery: PID 1 of item/grid's recovery on the Duo; not to be run here");
                std::process::exit(2);
            }
        },
    }
}
