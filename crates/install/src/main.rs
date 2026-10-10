//! itemgrid-install: item on a Surface Duo 1 that runs stock Android, from
//! this computer, in one go - by the same steps and safety rules as
//! item/grid's install (itemgrid-core: install::from_stock_by_usb), with
//! nothing but adb, fastboot and the USB: no ssh, no USB network.
//!
//! What it does, in order: adb and fastboot here (on Windows, fetched);
//! the newest system image release from GitHub, checked; an ssh key made
//! if there is none (so item/grid can reach the phone later); the phone
//! found over adb (USB debugging on) or in its bootloader; what will happen
//! said and confirmed; then the install, each step printed. The phone's
//! Android is erased by it.

mod fetch;

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("itemgrid-install [--yes] [--no-key] [--fetch-only]\n\n  item on a Surface Duo 1 from stock Android (USB debugging on), in one go.\n  --yes         do not ask before erasing the phone\n  --no-key      put no ssh key into the phone\n  --fetch-only  bring the tools and the image here, checked, and stop");
        return;
    }
    let code = match run(&args) {
        Ok(()) => 0,
        Err(e) => {
            println!("\nSTOPPED: {e}");
            1
        }
    };
    if !args.iter().any(|a| a == "--yes") {
        println!("\nPress Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}

fn say(line: String) {
    println!("  {line}");
    let _ = std::io::stdout().flush();
}

fn run(args: &[String]) -> Result<(), String> {
    println!("item/grid install - item on a Surface Duo 1\n");
    println!("1. adb and fastboot");
    fetch::tools(&mut say)?;
    say("ready".into());

    println!("2. the system image");
    let listed = fetch::newest_release()?;
    say(format!("newest: {}", listed.tag));
    let dir = fetch::release(&listed, &mut say)?;
    let release = itemgrid_core::install::read(&dir)?;
    // TWRP from the release folder (full::twrp looks in item/grid's own place).
    let twrp = dir.join("surfaceduo1-twrp.img");
    if twrp.exists() {
        std::env::set_var("ITEMGRID_TWRP", &twrp);
    }
    say(format!("item {} on the port {} ({})", release.item, release.adaptation, release.name));
    if args.iter().any(|a| a == "--fetch-only") {
        println!("\nFetched and checked: {}", dir.display());
        return Ok(());
    }

    println!("3. an ssh key for item/grid");
    let key = if args.iter().any(|a| a == "--no-key") { String::new() } else { ssh_key() };

    println!("4. the phone");
    say("plug the Duo in: in Android with USB debugging on (Settings > Developer options), or in its bootloader".into());
    let serial = find_phone()?;
    say(format!("found: Surface Duo {serial}"));

    println!("5. what happens now");
    println!("  - the phone's Android and everything on it are ERASED");
    println!("  - its bootloader is unlocked (you confirm that on the phone with the volume keys and Power)");
    println!("  - {} is put on, about 15 minutes on the cable; once during it you hold the power button when told", release.name);
    println!("  - afterwards item starts by itself; the PIN is 1234 until you choose one");
    if !args.iter().any(|a| a == "--yes") {
        print!("\nType yes to go on: ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if answer.trim() != "yes" {
            return Err("not confirmed - nothing is changed".into());
        }
    }
    println!("6. installing");
    let started = std::time::Instant::now();
    let mut stamped = |line: String| say(format!("[{:>4} s] {line}", started.elapsed().as_secs()));
    itemgrid_core::install::from_stock_by_usb(&serial, &release, &key, &mut stamped)?;
    println!("\nDone in {} minutes. The phone runs item now.", started.elapsed().as_secs() / 60);
    Ok(())
}

/// An ssh public key for the phone: one in ~/.ssh, else one made now with
/// ssh-keygen in item/grid's config folder (Windows 10 and later have it);
/// none if that is not possible either (the phone is still installed).
fn ssh_key() -> String {
    if let Ok(k) = itemgrid_core::install::public_key() {
        say("using the key in ~/.ssh".into());
        return k;
    }
    let dir = itemgrid_core::paths::config();
    let _ = std::fs::create_dir_all(&dir);
    let private = dir.join("id_ed25519");
    let public = dir.join("id_ed25519.pub");
    if !public.exists() {
        let made = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "itemgrid", "-f"])
            .arg(&private)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            say("no ssh-keygen here: no key goes in (item/grid will not reach this phone from this computer)".into());
            return String::new();
        }
    }
    match std::fs::read_to_string(&public) {
        Ok(k) if k.starts_with("ssh-") => {
            say(format!("using the key made at {}", public.display()));
            std::env::set_var("SFDUO_PUBKEY", &public);
            k.trim().to_owned()
        }
        _ => String::new(),
    }
}

/// The phone's serial: over adb (authorised), or in fastboot. Waits, saying
/// what is seen, until one is there.
fn find_phone() -> Result<String, String> {
    use std::process::Command;
    use std::time::Duration;
    let mut said = String::new();
    let mut tell = |s: String| {
        if s != said {
            say(s.clone());
            said = s;
        }
    };
    loop {
        let mut c = Command::new(itemgrid_core::programs::adb());
        c.arg("devices");
        let out = itemgrid_core::programs::output_within(c, Duration::from_secs(10)).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let mut lines = out.lines().skip(1).map(|l| l.split_whitespace().collect::<Vec<_>>()).filter(|w| w.len() >= 2);
        if let Some(w) = lines.find(|w| w[1] == "device") {
            return Ok(w[0].to_owned());
        }
        if out.contains("unauthorized") {
            tell("the phone asks whether to allow USB debugging from this computer: choose Allow on it".into());
        } else if out.contains("recovery") {
            tell("the phone is in a recovery: restart it into Android, or into its bootloader".into());
        }
        let mut c = Command::new(itemgrid_core::programs::fastboot());
        c.arg("devices");
        let out = itemgrid_core::programs::output_within(c, Duration::from_secs(10)).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if let Some(serial) = out.lines().find_map(|l| l.split_whitespace().next().filter(|s| !s.is_empty())) {
            return Ok(serial.to_owned());
        }
        tell("no phone seen yet (adb: USB debugging on, the cable in)".into());
        std::thread::sleep(Duration::from_secs(3));
    }
}
