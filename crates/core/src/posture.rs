//! The hinge's angle as it changes: the port's sfduo-posture (org.sfduo.Posture
//! on the owner's session bus) followed through one ssh, its PropertiesChanged
//! read as they come (up to sixty a second while the phone's display is lit),
//! the angle there first. The window folds its drawn Duo with them.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

const FOLLOW: &str = "busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Angle; busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Gravity 2>/dev/null; exec gdbus monitor --session --dest org.sfduo.Posture --object-path /org/sfduo/Posture";

/// What the phone says of itself as it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reading {
    /// The hinge, degrees: 180 flat.
    Angle(f64),
    /// Which way is up, in g, in the right half's frame (x across its
    /// panel, y along it toward its top, z out of its screen): lying flat
    /// face up about (0, 0, 1). From a port with Gravity in sfduo-posture.
    Gravity([f64; 3]),
}

/// The readings, one after another.
pub struct Follow {
    lines: BufReader<ChildStdout>,
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
    Ok((Follow { lines: BufReader::new(out) }, Stop(Arc::new(Mutex::new(child)))))
}

impl Follow {
    /// The next reading; None when the phone stopped answering.
    pub fn next(&mut self) -> Option<Reading> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.lines.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if let Some(r) = reading(&line) {
                return Some(r);
            }
        }
    }
}

/// A reading from busctl ("d 151", "ad 3 0.04 -0.01 1.02") or a
/// PropertiesChanged ('Angle': <151.0>, 'Gravity': <[0.04, -0.01, 1.02]>):
/// the gravity if the line has it, else the angle.
fn reading(line: &str) -> Option<Reading> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("d ") {
        return rest.trim().parse().ok().map(Reading::Angle);
    }
    if let Some(rest) = line.strip_prefix("ad 3 ") {
        let v: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        return (v.len() == 3).then(|| Reading::Gravity([v[0], v[1], v[2]]));
    }
    if let Some(at) = line.find("'Gravity': <[") {
        let rest = &line[at + 13..];
        let v: Vec<f64> = rest[..rest.find(']')?].split(',').filter_map(|x| x.trim().parse().ok()).collect();
        return (v.len() == 3).then(|| Reading::Gravity([v[0], v[1], v[2]]));
    }
    let rest = &line[line.find("'Angle': <")? + 10..];
    rest[..rest.find('>')?].trim().parse().ok().map(Reading::Angle)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_both_forms() {
        use super::{reading, Reading};
        assert_eq!(reading("d 151\n"), Some(Reading::Angle(151.0)));
        assert_eq!(reading("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Angle': <104.5>, 'Moving': <true>}, @as [])"), Some(Reading::Angle(104.5)));
        assert_eq!(reading("ad 3 0.0491367 -0.00868073 1.02411"), Some(Reading::Gravity([0.0491367, -0.00868073, 1.02411])));
        assert_eq!(reading("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Gravity': <[0.05, -0.5, 0.86]>}, @as [])"), Some(Reading::Gravity([0.05, -0.5, 0.86])));
        assert_eq!(reading("Monitoring signals on object"), None);
    }
}
