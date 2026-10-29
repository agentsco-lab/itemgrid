//! item-recovery: what item/grid boots on the Duo from RAM (fastboot boot,
//! nothing flashed) to put a system on it or take one off - the port's
//! kernel with an initramfs of this one program and a font. As PID 1 it
//! mounts what it needs, names the partitions as Android does
//! (/dev/block/.../by-name), raises the USB gadget with an adb function
//! under the phone's own serial and runs an adb daemon of its own: its
//! verbs with exit codes, push and pull, a stream into a verb or out of
//! one, reboot. Nothing else: no shell, no network, no touch, no Android.
//!
//! As `item-recovery VERB ...`: what item/grid asks of it (verbs.rs).

mod adb;
mod colour;
mod drm;
mod gpt;
mod init;
mod log;
mod mkfs;
mod screen;
mod verbs;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // The daemon, as init's child (init reaps orphans; the daemon
        // waits for its own children).
        Some("adbd") => adb::run(),
        // What item/grid asks of the recovery: verbs.rs.
        Some(v) if verbs::VERBS.contains(&v) => std::process::exit(verbs::run(v, &args[1..])),
        // adbd's answer to a line that is not a verb (adb.rs): there is
        // no shell here.
        Some("no-shell") => {
            eprintln!("item-recovery has no shell: item-recovery VERB ARGS, a verb of {}", verbs::VERBS.join(" "));
            std::process::exit(127);
        }
        // The screen's design as pictures, on a computer.
        Some("preview") => std::process::exit(screen::preview(args.get(1).map_or(".", String::as_str), args.get(2).map_or(screen::FONT, String::as_str))),
        _ if std::process::id() == 1 => init::run(),
        _ => {
            eprintln!("item-recovery: PID 1 of item/grid's recovery on the Duo; not to be run here");
            std::process::exit(2);
        }
    }
}
