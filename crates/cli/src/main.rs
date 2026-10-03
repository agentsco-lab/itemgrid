//! cradle: look after a connected Surface Duo from the computer.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => usage(),
        Some(other) => {
            eprintln!("cradle: '{other}' is not here yet");
            usage();
            std::process::exit(2);
        }
    }
}

fn usage() {
    println!("cradle - look after a connected Surface Duo\n");
    println!("Coming first:");
    for (cmd, what) in [
        ("status", "what the phone is doing; on Linux its versions, battery, heat, space, failed services"),
        ("update", "build item, install it, reboot, wait for the PIN"),
        ("logs", "the journal of this boot or one before, filtered, saved for a ticket"),
        ("shell / run", "a shell on the phone, or one command, with the dangerous ones refused"),
        ("reboot", "and wait until it is back"),
        ("screenshot", "both panels as one picture on the computer"),
    ] {
        println!("  {cmd:<12} {what}");
    }
}
