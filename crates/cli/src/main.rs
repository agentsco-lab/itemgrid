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

fn usage() {
    println!("cradle - look after a connected Surface Duo\n");
    println!("  status       what the phone is doing; on Linux its versions, battery, heat, space, failed services");
    println!("  update       build item, install it, reboot, wait until it runs (--no-build: install what is built)");
    println!("\nComing next:");
    for (cmd, what) in [
        ("logs", "the journal of this boot or one before, filtered, saved for a ticket"),
        ("shell / run", "a shell on the phone, or one command, with the dangerous ones refused"),
        ("reboot", "and wait until it is back"),
        ("screenshot", "both panels as one picture on the computer"),
    ] {
        println!("  {cmd:<12} {what}");
    }
}
