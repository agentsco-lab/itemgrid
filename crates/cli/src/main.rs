//! cradle: look after a connected Surface Duo from the computer.

use cradle_core::{status, Mode};

fn main() {
    // Output cut short (| head) ends cradle quietly, as other tools.
    // SAFETY: setting SIGPIPE's disposition before any thread runs.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => {
            usage();
            0
        }
        Some("status") => cmd_status(),
        Some("update") => cmd_update(&args[1..]),
        Some("logs") => cmd_logs(&args[1..]),
        Some("run") => cmd_run(&args[1..]),
        Some("shell") => cmd_shell(&args[1..]),
        Some("reboot") => cmd_reboot(),
        Some("backup") => cmd_backup(&args[1..]),
        Some("backups") => cmd_backups(),
        Some("slots") => cmd_slots(),
        Some("confirm") => cmd_confirm(&args[1..]),
        Some("ramboot") => cmd_ramboot(&args[1..]),
        Some("recovery-exit") => cmd_recovery_exit(),
        Some("club") => cmd_club(&args[1..]),
        Some("register") => cmd_register(),
        Some("restore") => cmd_restore(&args[1..]),
        Some("android") => cmd_android(&args[1..]),
        Some("screenshot") => cmd_screenshot(&args[1..]),
        Some(other) => {
            eprintln!("cradle: '{other}' is not here yet");
            usage();
            2
        }
    };
    std::process::exit(code);
}

fn cmd_status() -> i32 {
    let seen = cradle_core::detect();
    let via = if seen.via.is_empty() { String::new() } else { format!(" ({})", seen.via) };
    println!("Phone:     {}{via}", seen.mode.name());
    if seen.mode != Mode::Linux {
        println!("           {}", seen.mode.means());
        return if seen.mode == Mode::Gone { 1 } else { 0 };
    }
    let s = match status::read(&seen.via) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    if let Some(d) = cradle_core::club::known(&s.serial) {
        println!("Club:      {}{}", d.number, d.label.map(|l| format!(" - {l}")).unwrap_or_default());
    }
    let (h, m) = (s.uptime_s / 3600, s.uptime_s / 60 % 60);
    println!("System:    {}, kernel {}, up {h} h {m} min", s.os, s.kernel);
    println!("item:      {} (built {}), {}", s.item, s.item_built, if s.item_running { "running" } else { "not running" });
    println!("Port:      {}, sensorfw {}", s.port, s.sensorfw);
    let temp = s.battery_temp.map(|t| format!(", {t:.0} °C")).unwrap_or_default();
    println!("Battery:   {}%, {}{temp}", s.battery.map(|b| b.to_string()).unwrap_or("?".into()), s.battery_status.to_lowercase());
    if let Some(t) = s.cpu_temp {
        println!("CPU:       {t:.0} °C");
    }
    for (mount, size, free) in &s.disks {
        println!("Disk {mount:<9} {} free of {}", status::size_words(*free), status::size_words(*size));
    }
    if !s.failed.is_empty() {
        println!("Failed:    {}", s.failed.join(", "));
    }
    for w in s.warnings() {
        println!("! {w}");
    }
    0
}

fn cmd_update(args: &[String]) -> i32 {
    let build = !args.iter().any(|a| a == "--no-build");
    let seen = cradle_core::detect();
    if seen.mode != Mode::Linux {
        eprintln!("cradle: the phone is {}, not Linux: {}", seen.mode.name(), seen.mode.means());
        return 1;
    }
    let start = std::time::Instant::now();
    let result = cradle_core::update::update(&seen.via, build, &mut |step| {
        println!("[{:>4.0}s] {}", start.elapsed().as_secs_f64(), step.words());
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn cmd_logs(args: &[String]) -> i32 {
    use cradle_core::logs::Query;
    let mut q = Query { lines: Some(200), ..Default::default() };
    let mut save: Option<Option<String>> = None;
    let mut boots = false;
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} wants a value"));
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--boot" | "-b" => q.boot = value("--boot")?.parse().map_err(|_| "--boot wants a number, e.g. -1".to_owned())?,
                "--only" => q.only = Some(value("--only")?),
                "--grep" | "-g" => q.grep = Some(value("--grep")?),
                "--since" => q.since = Some(value("--since")?),
                "-n" => {
                    let n = value("-n")?;
                    q.lines = if n == "all" { None } else { Some(n.parse().map_err(|_| "-n wants a number or 'all'".to_owned())?) };
                }
                "-f" | "--follow" => q.follow = true,
                "--save" => save = Some(None),
                "--boots" => boots = true,
                other => return Err(format!("'{other}' is not a logs option")),
            }
            Ok(())
        })();
        if let Err(e) = r {
            eprintln!("cradle logs: {e}");
            return 2;
        }
        // A file after --save, if one is given.
        if a == "--save" {
            if let Some(next) = it.peek() {
                if !next.starts_with('-') {
                    save = Some(Some(it.next().cloned().unwrap()));
                }
            }
        }
    }
    let seen = cradle_core::detect();
    if seen.mode != Mode::Linux {
        eprintln!("cradle: the phone is {}, not Linux: {}", seen.mode.name(), seen.mode.means());
        return 1;
    }
    if boots {
        return cradle_core::phone::stream(&seen.via, cradle_core::logs::BOOTS).unwrap_or_else(|e| {
            eprintln!("cradle: {e}");
            1
        });
    }
    match save {
        None => cradle_core::phone::stream(&seen.via, &q.script()).unwrap_or_else(|e| {
            eprintln!("cradle: {e}");
            1
        }),
        Some(file) => {
            // Saved, the whole of what was asked for unless -n was given.
            if !args.iter().any(|a| a == "-n") {
                q.lines = None;
            }
            q.follow = false;
            let text = match cradle_core::phone::run(&seen.via, &q.script()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("cradle: {e}");
                    return 1;
                }
            };
            let path = file.unwrap_or_else(|| {
                let dir = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("cradle-logs");
                let _ = std::fs::create_dir_all(&dir);
                let stamp = std::process::Command::new("date").arg("+%Y-%m-%d-%H%M%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
                let part = q.only.clone().unwrap_or_else(|| "all".into());
                dir.join(format!("{stamp}-boot{}-{part}.log", q.boot)).display().to_string()
            });
            if let Err(e) = std::fs::write(&path, &text) {
                eprintln!("cradle: {path}: {e}");
                return 1;
            }
            println!("{} lines saved to {path}", text.lines().count());
            0
        }
    }
}

/// The phone, if Linux is up; said why not otherwise.
fn linux() -> Option<String> {
    let seen = cradle_core::detect();
    if seen.mode != Mode::Linux {
        eprintln!("cradle: the phone is {}, not Linux: {}", seen.mode.name(), seen.mode.means());
        return None;
    }
    Some(seen.via)
}

fn cmd_run(args: &[String]) -> i32 {
    let owner = args.iter().any(|a| a == "--user");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--user").collect();
    let script = match rest.as_slice() {
        [flag, path] if flag.as_str() == "--file" => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cradle run: {path}: {e}");
                return 2;
            }
        },
        [] => {
            eprintln!("cradle run: what to run? e.g. cradle run uptime, cradle run --file x.sh");
            return 2;
        }
        words => words.iter().map(|w| w.as_str()).collect::<Vec<_>>().join(" "),
    };
    // The check sees what was typed, before it is wrapped for the owner.
    if let Err(e) = cradle_core::guard::check(&script) {
        eprintln!("cradle: {e}");
        return 3;
    }
    let script = if owner { cradle_core::phone::as_owner(&script) } else { script };
    let Some(host) = linux() else { return 1 };
    cradle_core::phone::stream(&host, &script).unwrap_or_else(|e| {
        eprintln!("cradle: {e}");
        1
    })
}

fn cmd_shell(args: &[String]) -> i32 {
    let Some(host) = linux() else { return 1 };
    eprintln!("{}", cradle_core::guard::SHELL_WARNING);
    cradle_core::phone::shell(&host, args.iter().any(|a| a == "--user")).unwrap_or_else(|e| {
        eprintln!("cradle: {e}");
        1
    })
}

fn cmd_backup(args: &[String]) -> i32 {
    use cradle_core::backup::{self, Kind};
    let Some(host) = linux() else { return 1 };
    let serial = match backup::serial(&host) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    let kinds: Vec<Kind> = match args.first().map(String::as_str) {
        Some("device") => vec![Kind::Device],
        Some("boot") => vec![Kind::Boot],
        Some("quick") => vec![Kind::Quick],
        Some("full") => {
            let start = std::time::Instant::now();
            return match cradle_core::full::take(&host, &mut |line| println!("[{:>4.0}s] {line}", start.elapsed().as_secs_f64())) {
                Ok(b) => {
                    println!("        {}", b.dir.display());
                    0
                }
                Err(e) => {
                    eprintln!("cradle: STOP: {e}");
                    1
                }
            };
        }
        None | Some("all") => {
            // The device data once; the rest each time.
            let mut k = Vec::new();
            if !backup::has_device_data(&serial) {
                k.push(Kind::Device);
            }
            k.extend([Kind::Boot, Kind::Quick]);
            k
        }
        Some(other) => {
            eprintln!("cradle backup: '{other}' - device, boot, quick, full or all");
            return 2;
        }
    };
    let start = std::time::Instant::now();
    for kind in kinds {
        match backup::take(&host, kind, &mut |line| println!("[{:>4.0}s] {line}", start.elapsed().as_secs_f64())) {
            Ok(b) => println!("        {}", b.dir.display()),
            Err(e) => {
                eprintln!("cradle: {}: {e}", kind.words());
                return 1;
            }
        }
    }
    if let Some(device) = cradle_core::backup::list(Some(&serial)).into_iter().find(|b| b.manifest.kind == Kind::Device) {
        if !device.manifest.off_computer {
            println!("\nThe device data (radio calibration, IMEI, keys) exists nowhere but on the phone and here:");
            println!("copy {} to a USB drive or a cloud too.", device.dir.display());
        }
    }
    0
}

fn cmd_backups() -> i32 {
    let all = cradle_core::backup::list(None);
    if all.is_empty() {
        println!("No backups yet: cradle backup");
        return 0;
    }
    for b in all {
        let flags = [b.manifest.keep.then_some("kept"), b.manifest.off_computer.then_some("copied off")].into_iter().flatten().collect::<Vec<_>>().join(", ");
        println!("{}  {:<18} {:>9}  item {}  slot {}  {}", b.manifest.created, b.manifest.kind.words(), cradle_core::status::size_words(b.size() / 1024), b.manifest.item, b.manifest.slot, flags);
    }
    0
}

fn cmd_slots() -> i32 {
    let Some(host) = linux() else { return 1 };
    let slots = match cradle_core::slots::read(&host) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    for s in &slots {
        let mut flags = Vec::new();
        if s.active {
            flags.push("active".to_owned());
        }
        flags.push(if s.successful { "booted fine".into() } else { "never booted".into() });
        if s.unbootable {
            flags.push("UNBOOTABLE".into());
        }
        flags.push(format!("{} tries left", s.retries));
        println!("Slot {}:  {}", s.name.to_ascii_uppercase(), flags.join(", "));
        println!("         {}{}", s.image.clone().unwrap_or_else(|| "an image not known here".into()), if s.kernel.is_empty() { String::new() } else { format!(" · Linux {}", s.kernel) });
    }
    if let Ok(serial) = cradle_core::backup::serial(&host) {
        let g = cradle_core::flash::gate(&serial);
        println!(
            "RAM boots: {} of {} unconfirmed - {}",
            g.unconfirmed,
            cradle_core::flash::MAX_UNCONFIRMED,
            if g.open() { "the gate is open" } else { "the gate is CLOSED: confirm a good boot or reset it on purpose" }
        );
    }
    0
}

fn cmd_confirm(args: &[String]) -> i32 {
    let Some(host) = linux() else { return 1 };
    let (serial, ev) = match cradle_core::backup::serial(&host).and_then(|s| cradle_core::flash::evidence(&host).map(|e| (s, e))) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    println!("Booted from:     slot {}", ev.slot.to_ascii_uppercase());
    println!("Running kernel:  {}", ev.running_kernel);
    println!("Kernel in slot:  {}", ev.slot_kernel);
    println!("Image in slot:   {}", ev.image.clone().unwrap_or_else(|| "not known here".into()));
    if !ev.holds() {
        eprintln!("cradle: they differ - nothing is confirmed");
        return 1;
    }
    let g = cradle_core::flash::gate(&serial);
    println!("RAM boots now:   {} of {} unconfirmed", g.unconfirmed, cradle_core::flash::MAX_UNCONFIRMED);
    if !args.iter().any(|a| a == "--yes") {
        println!("\nThis records that the system running booted from slot {} and counts the RAM boots back to 0.", ev.slot.to_ascii_uppercase());
        println!("Nothing on the phone changes. Run again with --yes to record it.");
        return 0;
    }
    match cradle_core::flash::confirm_flashed(&serial, &ev) {
        Ok(()) => {
            println!("Recorded in {}: the RAM boots are 0 of {} now.", cradle_core::flash::state_path().display(), cradle_core::flash::MAX_UNCONFIRMED);
            0
        }
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn cmd_ramboot(args: &[String]) -> i32 {
    let Some(image) = args.iter().find(|a| !a.starts_with('-')) else {
        eprintln!("cradle ramboot: which image? e.g. cradle ramboot boot.img");
        return 2;
    };
    let image = std::path::Path::new(image);
    let Some(host) = linux() else { return 1 };
    let (img, serial, slot) = match cradle_core::ramboot::preflight(&host, image) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cradle: STOP: {e}");
            return 1;
        }
    };
    let gate = cradle_core::flash::gate(&serial);
    println!("Image:     {} ({} MB)", image.display(), img.size >> 20);
    println!("           header v2, ARM64 kernel, DTB, Android {}, hardware {}, sha {}", img.os_version, img.hardware, &img.sha256[..16]);
    println!("Phone:     booted from slot {}; RAM boots {} of {} unconfirmed", slot.to_ascii_uppercase(), gate.unconfirmed, cradle_core::flash::MAX_UNCONFIRMED);
    println!("Baseline:  {}", cradle_core::flash::baseline(&serial).map(|b| b.to_string()).unwrap_or_else(|| "none yet (taken from this boot)".into()));
    if !args.iter().any(|a| a == "--yes") {
        println!("\nThe checks that need no change passed. With --yes, in order (each a stop if it fails):");
        for step in [
            "1. the parking brake armed in misc from Linux, read back",
            "2. the phone into the bootloader (its screen goes to fastboot for about a minute)",
            "3. in fastboot: the same phone, unlocked, the same slot, the battery, the health against the baseline",
            "4. misc erased and the brake flashed again",
            "5. the attempt counted, then fastboot boot of the image - nothing flashed, the slots untouched",
            "6. Linux awaited; back: the boot confirmed and the brake armed again. Not back: stop - never the same image again",
        ] {
            println!("   {step}");
        }
        return 0;
    }
    let start = std::time::Instant::now();
    let expect = if args.iter().any(|a| a == "--recovery") { cradle_core::ramboot::Expect::Recovery } else { cradle_core::ramboot::Expect::of(image) };
    println!("Boots into: {}", if expect == cradle_core::ramboot::Expect::Recovery { "a recovery (awaited over adb)" } else { "Linux (awaited over ssh)" });
    match cradle_core::ramboot::ram_boot(&host, image, expect, &mut |line| println!("[{:>4.0}s] {line}", start.elapsed().as_secs_f64())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: STOP: {e}");
            1
        }
    }
}

fn cmd_recovery_exit() -> i32 {
    // The phone is in the recovery: no Linux to ask its serial; the last
    // one backed up or seen in the state is it.
    let serial = cradle_core::backup::list(None).first().map(|b| b.manifest.serial.clone());
    let Some(serial) = serial else {
        eprintln!("cradle: no phone known here yet");
        return 1;
    };
    let host = cradle_core::phone::hosts().into_iter().next().unwrap_or_default();
    let start = std::time::Instant::now();
    match cradle_core::ramboot::leave_recovery(&host, &serial, &mut |line| println!("[{:>4.0}s] {line}", start.elapsed().as_secs_f64())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn cmd_club(args: &[String]) -> i32 {
    use cradle_core::club;
    match args.first().map(String::as_str) {
        Some("token") => {
            println!("Paste the registry token from {} (Settings -> Device registry), then Enter:", club::server());
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                return 2;
            }
            match club::set_token(&line) {
                Ok(()) => {
                    println!("Kept in the keyring.");
                    0
                }
                Err(e) => {
                    eprintln!("cradle: {e}");
                    1
                }
            }
        }
        Some("forget") => match club::forget_token() {
            Ok(()) => {
                println!("The token is gone from the keyring.");
                0
            }
            Err(e) => {
                eprintln!("cradle: {e}");
                1
            }
        },
        _ => {
            println!("The club: {}", club::server());
            println!("Token: {}", if club::token().is_some() { "in the keyring" } else { "none - cradle club token" });
            0
        }
    }
}

fn cmd_register() -> i32 {
    let Some(host) = linux() else { return 1 };
    match cradle_core::club::register(&host) {
        Ok(d) => {
            println!("This Duo is {} in the club{}.", d.number, if d.new { " - newly registered" } else { "" });
            println!("Written on the phone: /etc/item/device-id");
            0
        }
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn cmd_restore(args: &[String]) -> i32 {
    let slot = match args.iter().position(|a| a == "--slot").and_then(|i| args.get(i + 1)).and_then(|s| s.chars().next()) {
        Some(c) => c.to_ascii_lowercase(),
        None => {
            eprintln!("cradle restore: which slot? --slot a or --slot b");
            return 2;
        }
    };
    let rewrite = args.iter().any(|a| a == "--rewrite");
    let dir = args.iter().enumerate().find(|(i, a)| !a.starts_with('-') && args.get(i.wrapping_sub(1)).is_none_or(|p| p != "--slot")).map(|(_, a)| std::path::PathBuf::from(a));
    let Some(host) = linux() else { return 1 };
    let serial = match cradle_core::backup::serial(&host) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    let Some(backup) = cradle_core::restore::pick(&serial, dir.as_deref()) else {
        eprintln!("cradle: no such boot-chain backup of this phone (cradle backups)");
        return 1;
    };
    let plan = match cradle_core::restore::plan(&host, &backup, slot, rewrite) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cradle: STOP: {e}");
            return 1;
        }
    };
    println!("Backup:  {} (item {}, slot {} then)", backup.manifest.created, backup.manifest.item, backup.manifest.slot);
    println!("Slot:    {}{}", slot.to_ascii_uppercase(), if slot == plan.active_slot { " - the one in use" } else { " - the spare" });
    for p in &plan.parts {
        let what = match (p.differs, p.write) {
            (true, _) => "differs: to be written",
            (false, true) => "the same: to be rewritten (--rewrite)",
            (false, false) => "the same: left alone",
        };
        println!("  {:<10} {what}", p.partition);
    }
    if plan.writes().is_empty() {
        println!("Nothing to write.");
        return 0;
    }
    if !args.iter().any(|a| a == "--yes") {
        println!("\nWith --yes: a changed boot tried from RAM first; the chain now backed up; each partition written with dd");
        println!("and read back; then, for the slot in use, a reboot into it. The other slot is not touched.");
        return 0;
    }
    let start = std::time::Instant::now();
    match cradle_core::restore::restore(&host, &plan, &mut |l| println!("[{:>4.0}s] {l}", start.elapsed().as_secs_f64())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: STOP: {e}");
            1
        }
    }
}

fn cmd_android(args: &[String]) -> i32 {
    use cradle_core::android;
    let yes = args.iter().any(|a| a == "--yes");
    let start = std::time::Instant::now();
    let mut say = |l: String| println!("[{:>4.0}s] {l}", start.elapsed().as_secs_f64());
    let done = |r: Result<(), String>| match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: STOP: {e}");
            1
        }
    };
    match args.first().map(String::as_str) {
        Some("start") | Some("back") => {
            let Some(serial) = android::away_serial() else {
                if android::port_without_system() {
                    eprintln!("cradle: the phone restarted into the port's kernel, which finds no system on the erased userdata.");
                    eprintln!("        Hold Power ~15 s until it is off, then Volume Down + Power for the bootloader; run this again.");
                } else {
                    eprintln!("cradle: no phone with a whole-system backup is on the USB (in Android it needs USB debugging on;");
                    eprintln!("        or Volume Down + Power from off, for the bootloader)");
                }
                return 1;
            };
            let host = cradle_core::phone::hosts().into_iter().next().unwrap_or_default();
            if args[0] == "start" {
                return done(android::start(&host, &serial, &mut say));
            }
            if !yes {
                println!("Back to Linux: TWRP from RAM, userdata made ext4 again (Android's data goes), the system and");
                println!("the Android container's data put back from the newest whole-system backup, checked part by part;");
                println!("misc cleared, the port started. About 30-40 minutes. Run again with --yes to go.");
                return 0;
            }
            return done(android::back(&host, &serial, &mut say));
        }
        Some("trial") => {
            // The way back tried alone, in TWRP: nothing is erased.
            let Some(serial) = android::away_serial() else {
                eprintln!("cradle: no phone with a whole-system backup is on the USB");
                return 1;
            };
            say("trying the way back (512 MB onto the phone, checked)".into());
            let r = android::try_the_way_back(&serial);
            if r.is_ok() {
                say("the way back works".into());
            }
            return done(r);
        }
        Some("go") | None => {}
        Some(other) => {
            eprintln!("cradle android: unknown '{other}' (go, start, back, trial)");
            return 2;
        }
    }
    let Some(host) = linux() else { return 1 };
    let plan = match android::plan(&host) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cradle: {e}");
            return 1;
        }
    };
    println!("Return to Android - the plan (nothing is changed)\n");
    for b in &plan.builds {
        println!("Android in super, slot {}: {} (vendor built {})", b.slot.to_ascii_uppercase(), b.fingerprint, android::when(b.vendor_utc));
    }
    match &plan.kernel {
        Some((k, b)) => println!("Stock kernel:   {} (built {}, {} min after its vendor) -> slot {}", k.path.display(), android::when(k.built_utc), (k.built_utc - b.vendor_utc) / 60, b.slot.to_ascii_uppercase()),
        None => {
            println!("Stock kernel:   none matching; seen:");
            for k in &plan.kernels_seen {
                println!("                {} built {}", k.path.display(), android::when(k.built_utc));
            }
        }
    }
    println!("Battery:        {}%", plan.battery.map(|b| b.to_string()).unwrap_or("?".into()));
    println!("Device data:    {}", if plan.device_data { "backed up" } else { "NOT backed up" });
    match &plan.full {
        Some(b) => println!("Whole system:   backed up {}{}", b.manifest.created, if plan.full_fresh { " - fresh" } else { " - not fresh: a new one is taken first (~18 min)" }),
        None => println!("Whole system:   no backup - one is taken first (~18 min)"),
    }
    if !plan.losses.is_empty() {
        println!("Lost with userdata (in no backup):");
        for (name, bytes) in &plan.losses {
            println!("                {name} ({})", cradle_core::status::size_words(bytes / 1024));
        }
    }
    if plan.stops.is_empty() {
        println!("\nNothing stops it.");
    } else {
        println!("\nWhat stops it:");
        for s in &plan.stops {
            println!("  - {s}");
        }
    }
    if args.first().map(String::as_str) != Some("go") || !yes || !plan.stops.is_empty() {
        println!("\n`cradle android go --yes`: the whole system backed up (or checked); TWRP from RAM; the way back");
        println!("tried; then, after you type the phone's number, metadata and userdata ERASED and stock Android");
        println!("started from RAM - as a guest: the port's kernel stays on the slot, so a plain restart finds no system;");
        println!("hold Volume Down + Power for the bootloader, then `cradle android start` runs Android again and");
        println!("`cradle android back` puts Linux back. Lost things need --accept-losses.");
        return if plan.stops.is_empty() { 0 } else { 1 };
    }
    let word = android::confirm_word(&cradle_core::backup::serial(&host).unwrap_or_default());
    println!("\nThis ERASES the phone's userdata. Type {word} to go on:");
    let mut typed = String::new();
    let _ = std::io::stdin().read_line(&mut typed);
    done(android::go(&host, &plan, &typed, args.iter().any(|a| a == "--accept-losses"), &mut say))
}

fn cmd_reboot() -> i32 {
    let Some(host) = linux() else { return 1 };
    let start = std::time::Instant::now();
    match cradle_core::phone::reboot(&host, &mut |b| println!("[{:>4.0}s] {}", start.elapsed().as_secs_f64(), b.words())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn cmd_screenshot(args: &[String]) -> i32 {
    let hinge = args.iter().any(|a| a == "--hinge");
    let path = args.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_else(|| {
        let dir = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("cradle-shots");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = std::process::Command::new("date").arg("+%Y-%m-%d-%H%M%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
        dir.join(format!("{stamp}.png")).display().to_string()
    });
    let Some(host) = linux() else { return 1 };
    let png = cradle_core::screenshot::take(&host).and_then(|rgba| cradle_core::screenshot::png(&rgba, hinge));
    match png.and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))) {
        Ok(()) => {
            println!("{path}");
            0
        }
        Err(e) => {
            eprintln!("cradle: {e}");
            1
        }
    }
}

fn usage() {
    println!("cradle - look after a connected Surface Duo\n");
    println!("  status       what the phone is doing; on Linux its versions, battery, heat, space, failed services");
    println!("  update       build item, install it, reboot, wait until it runs (--no-build: install what is built)");
    println!("  logs         the phone's journal: --boot -1, --only item|sensorfw|kernel|posture|pen|UNIT,");
    println!("               --grep PATTERN, --since TIME, -n N|all (200), -f, --save [FILE], --boots");
    println!("  run          a command (or --file SCRIPT) on the phone as root, or --user as its owner;");
    println!("               refused: the GPIO debug file, writing block devices");
    println!("  shell        a shell on the phone as root (--user: as its owner)");
    println!("  reboot       reboot the phone and wait until item runs again");
    println!("  backup       back up to ~/cradle-backups: device data (once), boot chain, home and settings;");
    println!("               or one: device | boot | quick. Reads only.");
    println!("               full: the whole system from TWRP (the phone in TWRP ~15-20 min, then back)");
    println!("  backups      the backups on this computer");
    println!("  slots        the two boot slots: their state and what is in them; the RAM boot gate");
    println!("  confirm      record that the running system booted from its slot (shows the evidence; --yes)");
    println!("  ramboot      try a boot image from RAM, by SAFETY.md's rules (shows the checks; --yes to go;");
    println!("               a TWRP image, or --recovery, is awaited in the recovery)");
    println!("  recovery-exit  out of the recovery, back into Linux");
    println!("  club         the Duo owners' club: club token (paste one from the site), club forget");
    println!("  register     this Duo's number in the club (00001...), written onto the phone");
    println!("  android      the plan for returning to the phone's stock Android (reads only);");
    println!("               go --yes: there (erases userdata; asks for the phone's number); start: Android again;");
    println!("               back --yes: Linux again, from the whole-system backup");
    println!("  restore      a slot's boot chain from a backup: [BACKUP] --slot a|b (shows the plan; --yes;");
    println!("               --rewrite writes the same bytes, to try the writing on the spare slot)");
    println!("  screenshot   both panels as one PNG ([FILE], ~/cradle-shots/ by default; --hinge keeps its strip)");
}
