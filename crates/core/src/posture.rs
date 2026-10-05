//! The hinge's angle as it changes: the port's sfduo-posture (org.sfduo.Posture
//! on the owner's session bus) followed through one ssh, its PropertiesChanged
//! read as they come (up to sixty a second while the phone's display is lit),
//! the angle there first. The window folds its drawn Duo with them.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

// The monitor in the background, and a heartbeat line every 10 s: once
// the window is gone the heartbeat cannot be written, and the monitor is
// stopped (killing ssh here leaves the far side running: it would only
// notice at its next write, which the dark never brings).
const FOLLOW: &str = "busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Angle; busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Gravity 2>/dev/null; busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Posture
gdbus monitor --session --dest org.sfduo.Posture --object-path /org/sfduo/Posture & m=$!
trap '' PIPE
while kill -0 $m 2>/dev/null; do sleep 10; echo . || break; done
kill $m 2>/dev/null";

/// What the phone says of itself as it moves.
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// The hinge, degrees: 180 flat.
    Angle(f64),
    /// Which way is up, in g, in the right half's frame (x across its
    /// panel, y along it toward its top, z out of its screen): lying flat
    /// face up about (0, 0, 1). From a port with Gravity in sfduo-posture.
    Gravity([f64; 3]),
    /// The posture by the hinge: closed, laptop, flat, folded or between.
    Posture(String),
}

/// The readings, one after another.
pub struct Follow {
    lines: BufReader<ChildStdout>,
    /// A line's readings not yet given (a change can carry several).
    queued: std::collections::VecDeque<Reading>,
}

/// Stops following, from anywhere.
#[derive(Clone)]
pub struct Stop(Arc<Mutex<Child>>);

impl Stop {
    /// Whether two stops stop the same following.
    pub fn same(&self, other: &Stop) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn stop(&self) {
        let _ = self.0.lock().map(|mut c| c.kill());
    }
}

/// Starts following the hinge on the phone at `host`.
pub fn follow(host: &str) -> Result<(Follow, Stop), String> {
    let mut child = crate::phone::spawn(host, &crate::phone::as_owner(FOLLOW), Stdio::piped())?;
    let out = child.stdout.take().ok_or("no output")?;
    Ok((Follow { lines: BufReader::new(out), queued: Default::default() }, Stop(Arc::new(Mutex::new(child)))))
}

impl Follow {
    /// The next reading; None when the phone stopped answering.
    pub fn next(&mut self) -> Option<Reading> {
        let mut line = String::new();
        loop {
            if let Some(r) = self.queued.pop_front() {
                return Some(r);
            }
            line.clear();
            if self.lines.read_line(&mut line).ok()? == 0 {
                return None;
            }
            self.queued.extend(readings(&line));
        }
    }
}

/// The readings in a line from busctl ("d 151", "ad 3 0.04 -0.01 1.02",
/// "s \"laptop\"") or a PropertiesChanged ('Angle': <151.0>, 'Gravity':
/// <[0.04, -0.01, 1.02]>, 'Posture': <'laptop'>), all it has.
fn readings(line: &str) -> Vec<Reading> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("d ") {
        return rest.trim().parse().ok().map(Reading::Angle).into_iter().collect();
    }
    if let Some(rest) = line.strip_prefix("s \"") {
        return vec![Reading::Posture(rest.trim_end_matches('"').to_owned())];
    }
    let three = |text: &str, sep: char| -> Option<Reading> {
        let v: Vec<f64> = text.split(sep).filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 3).then(|| Reading::Gravity([v[0], v[1], v[2]]))
    };
    if let Some(rest) = line.strip_prefix("ad 3 ") {
        return three(rest, ' ').into_iter().collect();
    }
    let mut out = Vec::new();
    let after = |key: &str| line.find(key).map(|at| &line[at + key.len()..]);
    if let Some(rest) = after("'Angle': <") {
        if let Some(a) = rest.find('>').and_then(|e| rest[..e].trim().parse().ok()) {
            out.push(Reading::Angle(a));
        }
    }
    if let Some(rest) = after("'Gravity': <[") {
        if let Some(g) = rest.find(']').and_then(|e| three(&rest[..e], ',')) {
            out.push(g);
        }
    }
    if let Some(rest) = after("'Posture': <'") {
        if let Some(e) = rest.find('\'') {
            out.push(Reading::Posture(rest[..e].to_owned()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_both_forms() {
        use super::{readings, Reading};
        let reading = |l: &str| readings(l).into_iter().next();
        assert_eq!(reading("d 151\n"), Some(Reading::Angle(151.0)));
        assert_eq!(reading("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Angle': <104.5>, 'Moving': <true>}, @as [])"), Some(Reading::Angle(104.5)));
        assert_eq!(reading("ad 3 0.0491367 -0.00868073 1.02411"), Some(Reading::Gravity([0.0491367, -0.00868073, 1.02411])));
        assert_eq!(reading("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Gravity': <[0.05, -0.5, 0.86]>}, @as [])"), Some(Reading::Gravity([0.05, -0.5, 0.86])));
        assert_eq!(reading("s \"laptop\""), Some(Reading::Posture("laptop".into())));
        assert_eq!(reading("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Posture': <'closed'>}, @as [])"), Some(Reading::Posture("closed".into())));
        assert_eq!(reading("Monitoring signals on object"), None);
        assert_eq!(readings("('org.sfduo.Posture', {'Angle': <88.0>, 'Posture': <'laptop'>}, @as [])"), vec![Reading::Angle(88.0), Reading::Posture("laptop".into())]);
    }
}
