//! The hinge's angle as it changes: the port's sfduo-posture (org.sfduo.Posture
//! on the owner's session bus) followed through one ssh, its PropertiesChanged
//! read as they come (up to sixty a second while the phone's display is lit),
//! the angle there first. The window folds its drawn Duo with them.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

const FOLLOW: &str = "busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Angle; exec gdbus monitor --session --dest org.sfduo.Posture --object-path /org/sfduo/Posture";

/// The angles, one after another.
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
    /// The next angle; None when the phone stopped answering.
    pub fn next(&mut self) -> Option<f64> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.lines.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if let Some(a) = angle(&line) {
                return Some(a);
            }
        }
    }
}

/// An angle from busctl ("d 151") or a PropertiesChanged ('Angle': <151.0>).
fn angle(line: &str) -> Option<f64> {
    if let Some(rest) = line.trim().strip_prefix("d ") {
        return rest.trim().parse().ok();
    }
    let rest = &line[line.find("'Angle': <")? + 10..];
    rest[..rest.find('>')?].trim().parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_both_forms() {
        assert_eq!(super::angle("d 151\n"), Some(151.0));
        assert_eq!(super::angle("/org/sfduo/Posture: org.freedesktop.DBus.Properties.PropertiesChanged ('org.sfduo.Posture', {'Angle': <104.5>, 'Moving': <true>}, @as [])"), Some(104.5));
        assert_eq!(super::angle("Monitoring signals on object"), None);
    }
}
