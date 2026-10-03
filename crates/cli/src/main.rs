//! cradle: look after a connected Surface Duo from the computer.

use cradle_core::{status, Mode};

fn main() {
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

fn usage() {
    println!("cradle - look after a connected Surface Duo\n");
    println!("  status       what the phone is doing; on Linux its versions, battery, heat, space, failed services");
    println!("  update       build item, install it, reboot, wait until it runs (--no-build: install what is built)");
    println!("  logs         the phone's journal: --boot -1, --only item|sensorfw|kernel|posture|pen|UNIT,");
    println!("               --grep PATTERN, --since TIME, -n N|all (200), -f, --save [FILE], --boots");
    println!("  run          a command (or --file SCRIPT) on the phone as root, or --user as its owner;");
    println!("               refused: the GPIO debug file, writing block devices");
    println!("  shell        a shell on the phone as root (--user: as its owner)");
    println!("\nComing next:");
    for (cmd, what) in [
        ("reboot", "and wait until it is back"),
        ("screenshot", "both panels as one picture on the computer"),
    ] {
        println!("  {cmd:<12} {what}");
    }
}
