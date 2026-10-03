//! The phone's journal: a boot, a part of the system, a pattern, a time -
//! read once, saved, or followed.

/// What to read.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// 0 this boot, -1 the one before, and so on.
    pub boot: i32,
    /// One part: item, sensorfw, kernel, posture, pen, or a unit's name.
    pub only: Option<String>,
    pub grep: Option<String>,
    pub since: Option<String>,
    /// The last lines only; none for all of them.
    pub lines: Option<u32>,
    pub follow: bool,
}

/// The parts known by name, as journalctl's matches.
fn part(name: &str) -> Vec<String> {
    let tags: &[&str] = match name {
        "item" => &["item", "item-compositor", "item-session", "item-face", "item-composer-fresh", "pen-split"],
        "sensorfw" => &["sensorfwd"],
        "posture" => &["sfduo-posture", "sfduo-posture.desktop"],
        "pen" => &["pen-split", "sfduo-pen-split"],
        "kernel" => return vec!["-k".to_owned()],
        unit => return vec!["-u".to_owned(), quote(unit)],
    };
    tags.iter().flat_map(|t| ["-t".to_owned(), quote(t)]).collect()
}

/// A word for sh, whatever is in it.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl Query {
    /// The script that reads it on the phone.
    pub fn script(&self) -> String {
        let mut args = vec!["journalctl".to_owned(), "--no-pager".to_owned(), "-o".to_owned(), "short-precise".to_owned()];
        args.push(format!("-b{}", if self.boot == 0 { String::new() } else { self.boot.to_string() }));
        if let Some(only) = &self.only {
            args.extend(part(only));
        }
        if let Some(g) = &self.grep {
            args.push("--grep".to_owned());
            args.push(quote(g));
        }
        if let Some(s) = &self.since {
            args.push("--since".to_owned());
            args.push(quote(s));
        }
        if self.follow {
            args.push("-f".to_owned());
            if let Some(n) = self.lines {
                args.push(format!("-n{n}"));
            }
            return format!("exec {}\n", args.join(" "));
        }
        match self.lines {
            // With --grep, journalctl's -n gives the newest first: the last
            // lines are taken after it instead.
            Some(n) if self.grep.is_some() => format!("{} | tail -n {n}\n", args.join(" ")),
            Some(n) => format!("exec {} -n{n}\n", args.join(" ")),
            None => format!("exec {}\n", args.join(" ")),
        }
    }
}

/// The boots the journal keeps, as journalctl lists them.
pub const BOOTS: &str = "exec journalctl --list-boots --no-pager\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_what_is_typed() {
        let q = Query { grep: Some("a'; reboot; '".into()), ..Default::default() };
        assert!(q.script().contains(r"--grep 'a'\''; reboot; '\'''"));
    }

    #[test]
    fn a_part_and_a_boot() {
        let q = Query { boot: -1, only: Some("item".into()), lines: Some(50), ..Default::default() };
        let s = q.script();
        assert!(s.contains("-b-1") && s.contains("-t 'item-compositor'") && s.contains("-n50"));
    }
}
