//! The two commands item/grid's scripts use that the port's busybox has
//! not, as small as they need to be.

/// `truncate -s SIZE FILE...` (bytes, or with K, M, G): each file made
/// that long, created if it is not there.
pub fn truncate(args: &[String]) -> i32 {
    let mut size: Option<u64> = None;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-s" {
            size = it.next().and_then(|v| parse_size(v));
        } else if let Some(v) = a.strip_prefix("-s") {
            size = parse_size(v);
        } else {
            files.push(a);
        }
    }
    let Some(size) = size else {
        eprintln!("truncate -s SIZE FILE...");
        return 1;
    };
    let mut rc = 0;
    for f in files {
        let r = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(f).and_then(|h| h.set_len(size));
        if let Err(e) = r {
            eprintln!("truncate: {f}: {e}");
            rc = 1;
        }
    }
    rc
}

fn parse_size(v: &str) -> Option<u64> {
    let (n, k) = match v.chars().last()? {
        'K' | 'k' => (&v[..v.len() - 1], 1 << 10),
        'M' | 'm' => (&v[..v.len() - 1], 1 << 20),
        'G' | 'g' => (&v[..v.len() - 1], 1 << 30),
        _ => (v, 1),
    };
    n.parse::<u64>().ok().map(|n| n * k)
}

/// `mountpoint [-q] DIR`: 0 if something is mounted there (as
/// /proc/mounts has it), 1 if not.
pub fn mountpoint(args: &[String]) -> i32 {
    let quiet = args.iter().any(|a| a == "-q");
    let Some(dir) = args.iter().find(|a| !a.starts_with('-')) else {
        eprintln!("mountpoint [-q] DIR");
        return 1;
    };
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.into());
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let yes = mounts.lines().any(|l| l.split_whitespace().nth(1).is_some_and(|m| std::path::Path::new(m) == real));
    if !quiet {
        println!("{dir} is {}a mountpoint", if yes { "" } else { "not " });
    }
    if yes {
        0
    } else {
        1
    }
}
