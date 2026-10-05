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
// The lid's switch too (logind's LidClosed, on the system bus): told at
// once, where the hinge is only read with the display lit - opened, its
// angle came seconds later, or not at all before the display was on.
const FOLLOW: &str = "busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Angle; busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Gravity 2>/dev/null; busctl --user get-property org.sfduo.Posture /org/sfduo/Posture org.sfduo.Posture Posture
busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager LidClosed 2>/dev/null
gdbus monitor --session --dest org.sfduo.Posture --object-path /org/sfduo/Posture & m=$!
gdbus monitor --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1 | grep --line-buffered LidClosed & l=$!
sleep 0.3; s=$(pgrep -P $$ -x gdbus | grep -vx \"$m\")
trap '' PIPE
while kill -0 $m 2>/dev/null; do sleep 10; echo . || break; done
kill $m $s $l 2>/dev/null";

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
    /// The lid's switch: shut or not.
    Lid(bool),
    /// duo-motion's protocol version (its first line).
    Version(u32),
    /// The phone's orientation from duo-motion: a quaternion (w, x, y, z),
    /// the right half's frame to the world's (x to magnetic north, z up).
    Quat([f64; 4]),
    /// Whether duo-motion heeds the field just now (else the gyroscope
    /// alone carries the turn about the vertical).
    North(bool),
    /// The phone looked at: the way its screen faces about the world's z,
    /// from north, counter-clockwise (rad) - where the one looking at it is.
    Look(f64),
}

/// Where duo-motion is kept on the phone.
const MOTION_ON_PHONE: &str = "/var/lib/itemgrid/duo-motion";

/// The phone kept from sleeping while what follows runs (as root: item's
/// own asking to sleep is refused).
const INHIBIT: &str = "systemd-inhibit --what=sleep --who=item/grid --why='Following the phone on the cable' --mode=block";

/// duo-motion as built for the phone, here: ITEMGRID_MOTION, else
/// ~/.local/share/itemgrid/duo-motion (tools/install-local.sh puts it there).
fn motion_here() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("ITEMGRID_MOTION").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/itemgrid/duo-motion"));
    p.exists().then_some(p)
}

/// Follows the phone through duo-motion (its own sensor sessions, the
/// orientation fused on the phone, 50 a second; the lid from its switch):
/// put on the phone first when it is not there or not this one. None when
/// there is no duo-motion here (the window then follows sfduo-posture).
pub fn follow_motion(host: &str, awake: bool) -> Option<Result<(Follow, Stop), String>> {
    use sha2::{Digest, Sha256};
    let local = motion_here()?;
    Some((|| {
        let bytes = std::fs::read(&local).map_err(|e| format!("{}: {e}", local.display()))?;
        let want = format!("{:x}", Sha256::digest(&bytes));
        let have = crate::phone::run(host, &format!("sha256sum {MOTION_ON_PHONE} 2>/dev/null | cut -c1-64\n"))?;
        if have.trim() != want {
            // Where it was kept under the old names (Cradle, Hythe, Gridbay), let go.
            crate::phone::run_checked(host, "mkdir -p /var/lib/itemgrid && rm -rf /var/lib/cradle /var/lib/hythe /var/lib/gridbay\n")?;
            crate::phone::put(host, &local, &format!("{MOTION_ON_PHONE}.new"))?;
            crate::phone::run_checked(host, &format!("chmod 755 {MOTION_ON_PHONE}.new && mv {MOTION_ON_PHONE}.new {MOTION_ON_PHONE}\n"))?;
        }
        let script = if awake { format!("exec {INHIBIT} {MOTION_ON_PHONE} --cable\n") } else { format!("exec {MOTION_ON_PHONE}\n") };
        let mut child = crate::phone::spawn_own(host, &script, Stdio::piped())?;
        let out = child.stdout.take().ok_or("no output")?;
        Ok((Follow { lines: BufReader::new(out), queued: Default::default() }, Stop(Arc::new(Mutex::new(child)))))
    })())
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

/// Starts following the hinge on the phone at `host`; `awake`: the phone
/// kept from sleeping as long as it is followed (logind's inhibitor, let go
/// when the following ends - the window closed or the cable out): asleep,
/// the lid and the hinge reached the window only after it woke, the link
/// came back and the window looked again - seconds.
/// The phone's Wi-Fi power saving off for as long as the script that
/// starts with this runs (as root).
const WIFI_AWAKE: &str = "iw dev wlan0 set power_save off 2>/dev/null\ntrap 'iw dev wlan0 set power_save on 2>/dev/null' EXIT\ntrap 'exit 0' HUP INT TERM PIPE\n";

pub fn follow(host: &str, awake: bool) -> Result<(Follow, Stop), String> {
    // As root for the inhibitor; the following itself as the owner (in a
    // subshell: as_owner ends in exec).
    let script = if awake {
        // logind's sleep inhibitor, held while it runs: a kernel wakelock
        // does not stop systemd-sleep (item asked, the phone went down - the
        // USB link with it - and woke again, round and round). It goes with
        // its holder: killed, nothing is left.
        format!("exec {INHIBIT} sh -s <<'ITEMGRID_FOLLOW'\n( {} )\nITEMGRID_FOLLOW\n", crate::phone::as_owner(FOLLOW))
    } else {
        // On Wi-Fi: its power saving off while followed (on, the radio
        // dozed between packets: 7-186 ms a reading, 4 ms without), on
        // again however it ends - stopped, the link gone, asleep.
        format!("{WIFI_AWAKE}( {} )\n", crate::phone::as_owner(FOLLOW))
    };
    let mut child = crate::phone::spawn_own(host, &script, Stdio::piped())?;
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
    // duo-motion's: "v 2", "q w x y z", "n 0|1", "g x y z", "h deg", "l 0|1",
    // "look rad".
    let nums = |rest: &str| rest.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<f64>>();
    if let Some(rest) = line.strip_prefix("q ") {
        let v = nums(rest);
        return if v.len() == 4 { vec![Reading::Quat([v[0], v[1], v[2], v[3]])] } else { vec![] };
    }
    if let Some(rest) = line.strip_prefix("g ") {
        let v = nums(rest);
        return if v.len() == 3 { vec![Reading::Gravity([v[0], v[1], v[2]])] } else { vec![] };
    }
    if let Some(rest) = line.strip_prefix("v ") {
        return rest.trim().parse().ok().map(Reading::Version).into_iter().collect();
    }
    if let Some(rest) = line.strip_prefix("n ") {
        return vec![Reading::North(rest.trim() == "1")];
    }
    if let Some(rest) = line.strip_prefix("look ") {
        return nums(rest).first().map(|a| Reading::Look(*a)).into_iter().collect();
    }
    if let Some(rest) = line.strip_prefix("h ") {
        return nums(rest).first().map(|a| Reading::Angle(*a)).into_iter().collect();
    }
    if let Some(rest) = line.strip_prefix("l ") {
        return vec![Reading::Lid(rest.trim() == "1")];
    }
    if let Some(rest) = line.strip_prefix("d ") {
        return rest.trim().parse().ok().map(Reading::Angle).into_iter().collect();
    }
    if let Some(rest) = line.strip_prefix("b ") {
        return vec![Reading::Lid(rest.trim() == "true")];
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
    if let Some(rest) = after("'LidClosed': <") {
        out.push(Reading::Lid(rest.starts_with("true")));
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
        assert_eq!(reading("b true"), Some(Reading::Lid(true)));
        assert_eq!(reading("/org/freedesktop/login1: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.login1.Manager', {'LidClosed': <false>}, @as [])"), Some(Reading::Lid(false)));
        assert_eq!(readings("('org.sfduo.Posture', {'Angle': <88.0>, 'Posture': <'laptop'>}, @as [])"), vec![Reading::Angle(88.0), Reading::Posture("laptop".into())]);
    }
}
