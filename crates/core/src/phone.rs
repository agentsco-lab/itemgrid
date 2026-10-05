//! The phone over ssh: scripts are sent on the shell's standard input
//! (`sh -s`), never on its command line - a `pkill -f` pattern on the command
//! line matches the remote shell itself and kills it.

use std::io::Write;
use std::process::{Command, Stdio};

/// The addresses tried, in order: the USB link first, then where the phones
/// seen on the cable were on the network (link.rs).
pub fn hosts() -> Vec<String> {
    let mut hosts = vec![crate::link::CABLE.to_owned()];
    hosts.extend(crate::link::wifi_hosts());
    hosts
}

fn ssh(host: &str, connect_timeout: u32) -> Command {
    ssh_with(host, connect_timeout, true)
}

/// ssh to the phone; `shared`: through one connection kept for a minute
/// after its last use (a command then costs a few ms, not a new handshake),
/// noticed dead within ~6 s.
fn ssh_with(host: &str, connect_timeout: u32, shared: bool) -> Command {
    let mut c = Command::new("ssh");
    // The phone's host key changes with each image: it is not written into
    // the owner's known_hosts. On the cable (a link to the phone alone) it
    // is not checked; over Wi-Fi it must be the one the cable showed for that
    // phone (link.rs), else no connection - an address not found for a known
    // phone has no key to match.
    c.args(["-o", "BatchMode=yes", "-o", &format!("ConnectTimeout={connect_timeout}"), "-o", "LogLevel=ERROR"]);
    c.args(host_key_args(host));
    if shared {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        c.args(["-o", "ControlMaster=auto", "-o", &format!("ControlPath={dir}/itemgrid-%C"), "-o", "ControlPersist=60", "-o", "ServerAliveInterval=2", "-o", "ServerAliveCountMax=3"]);
    } else {
        // Its own: noticed dead within ~6 s too (a phone asleep on Wi-Fi
        // left it hanging for the system's own timeout, ~40 s).
        c.args(["-o", "ControlMaster=no", "-o", "ControlPath=none", "-o", "ServerAliveInterval=2", "-o", "ServerAliveCountMax=3"]);
    }
    c.arg(format!("root@{host}"));
    c
}

/// How ssh (and scp) check the phone's host key at `host`.
pub fn host_key_args(host: &str) -> Vec<String> {
    let o = |v: String| ["-o".to_owned(), v];
    if host == crate::link::CABLE && crate::link::usb_up() {
        return [o("UserKnownHostsFile=/dev/null".into()), o("StrictHostKeyChecking=no".into())].concat();
    }
    let alias = crate::link::serial_of(host).map(|s| format!("itemgrid-{s}")).unwrap_or_else(|| "itemgrid-unknown".into());
    [o(format!("UserKnownHostsFile={}", crate::link::known_hosts_path().display())), o("StrictHostKeyChecking=yes".into()), o(format!("HostKeyAlias={alias}")), o("GlobalKnownHostsFile=/dev/null".into())].concat()
}

/// Whether the phone answers ssh at `host`.
pub fn answers(host: &str) -> bool {
    if host == crate::link::CABLE && !crate::link::usb_up() {
        return false;
    }
    ssh(host, 2).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The same, over a new connection: while the phone reboots, a kept one may
/// not know yet that it is dead.
pub fn answers_fresh(host: &str) -> bool {
    if host == crate::link::CABLE && !crate::link::usb_up() {
        return false;
    }
    ssh_with(host, 2, false).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The kept connection to `host` closed (before a reboot).
pub fn close_shared(host: &str) {
    let _ = ssh(host, 2).args(["-O", "exit"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// The fingers the reader knows (droidian-fpd's GetAll), when it answers.
pub fn fingers(host: &str) -> Option<u32> {
    let out = run(host, "busctl --system call org.droidian.fingerprint /org/droidian/fingerprint org.droidian.fingerprint GetAll 2>/dev/null\n").ok()?;
    out.trim().strip_prefix("as ")?.split_whitespace().next()?.parse().ok()
}

/// A script run on the phone as root; its standard output.
pub fn run(host: &str, script: &str) -> Result<String, String> {
    run_bytes(host, script).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A script run on the phone as root, which must succeed: its output, or
/// the end of what it said when it fails.
pub fn run_checked(host: &str, script: &str) -> Result<String, String> {
    crate::guard::check(script)?;
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let said = format!("{}{}", text, String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = said.lines().rev().take(4).collect();
        return Err(tail.into_iter().rev().collect::<Vec<_>>().join(" / "));
    }
    Ok(text)
}

/// A script run on the phone as root; its standard output as bytes.
pub fn run_bytes(host: &str, script: &str) -> Result<Vec<u8>, String> {
    crate::guard::check(script)?;
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    if out.status.code() == Some(255) {
        return Err(format!("ssh: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// A file from here written on the phone at `remote` (root's), through the
/// same link as every command - its bytes on ssh's standard input (scp
/// asked for a new connection, which Wi-Fi's shared one did not give).
pub fn put(host: &str, local: &std::path::Path, remote: &str) -> Result<(), String> {
    if !remote.starts_with('/') || remote.contains(['\'', '\n', ' ']) {
        return Err("not a plain path on the phone".into());
    }
    let script = format!("cat > '{remote}'");
    crate::guard::check(&script)?;
    let file = std::fs::File::open(local).map_err(|e| format!("{}: {e}", local.display()))?;
    let out = ssh(host, 4).arg(&script).stdin(file).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("ssh: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("copying to the phone: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Where a reboot is, for whoever shows it.
#[derive(Debug, Clone)]
pub enum Boot {
    Rebooting,
    Down,
    Up,
    ItemRunning,
}

impl Boot {
    pub fn words(&self) -> &'static str {
        match self {
            Boot::Rebooting => "rebooting the phone",
            Boot::Down => "the phone went down; waiting for it to come back",
            Boot::Up => "the phone is back; waiting for item",
            Boot::ItemRunning => "item is running: enter the PIN",
        }
    }
}

/// The phone rebooted, and waited for: down, back, item running.
pub fn reboot(host: &str, step: &mut dyn FnMut(Boot)) -> Result<(), String> {
    use std::time::Duration;
    step(Boot::Rebooting);
    // The connection drops as the phone goes down: its error is expected.
    let _ = run(host, "sync; systemctl reboot");
    close_shared(host);
    wait(|| !answers_fresh(host), Duration::from_secs(90), "the phone did not go down")?;
    step(Boot::Down);
    wait(|| answers_fresh(host), Duration::from_secs(300), "the phone did not come back within 5 minutes")?;
    step(Boot::Up);
    wait(
        || run(host, "systemctl is-active item.service").is_ok_and(|s| s.trim() == "active"),
        Duration::from_secs(120),
        "item did not start within 2 minutes - `itemgrid logs` shows why",
    )?;
    step(Boot::ItemRunning);
    Ok(())
}

fn wait(mut done: impl FnMut() -> bool, limit: std::time::Duration, fail: &str) -> Result<(), String> {
    let start = std::time::Instant::now();
    while start.elapsed() < limit {
        if done() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Err(fail.to_owned())
}

/// A script run on the phone as root, its output going straight to ours
/// (for what is followed or long); its exit code.
pub fn stream(host: &str, script: &str) -> Result<i32, String> {
    crate::guard::check(script)?;
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).spawn().map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    let status = child.wait().map_err(|e| format!("ssh: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// A script made to run as the phone's owner (the user logged in), with
/// their session bus - for gdbus --session, systemctl --user and the like.
/// The phone's owner found: $U their name, $I their uid.
pub const OWNER: &str = r#"U=$(loginctl list-users --no-legend 2>/dev/null | awk '$2 != "root" {print $2; exit}'); U=${U:-droidian}; I=$(id -u "$U")"#;

pub fn as_owner(script: &str) -> String {
    let user = OWNER;
    format!(
        "{user}\nexec sudo -u \"$U\" env XDG_RUNTIME_DIR=/run/user/$I DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$I/bus sh -c {}\n",
        crate::logs::quote(script)
    )
}

/// An interactive shell on the phone, as root or as its owner; its exit code.
pub fn shell(host: &str, owner: bool) -> Result<i32, String> {
    let mut c = ssh(host, 4);
    c.arg("-t");
    if owner {
        c.arg(as_owner("exec bash -l"));
    }
    let status = c.status().map_err(|e| format!("ssh: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// A script started on the phone, its output piped back (for what streams);
/// the child, to be read and stopped.
pub fn spawn(host: &str, script: &str, out: Stdio) -> Result<std::process::Child, String> {
    spawn_with(host, script, out, true)
}

/// The same over its own connection, for what runs as long as the window
/// lives (following the phone): through the shared one, the window gone
/// (killed, crashed), the session went on on the phone for ever - and
/// what it held with it (the phone kept from sleeping). Alone, ssh notices
/// its output gone at the next line and ends.
pub fn spawn_own(host: &str, script: &str, out: Stdio) -> Result<std::process::Child, String> {
    spawn_with(host, script, out, false)
}

fn spawn_with(host: &str, script: &str, out: Stdio, shared: bool) -> Result<std::process::Child, String> {
    crate::guard::check(script)?;
    let mut child = ssh_with(host, 4, shared).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(out).stderr(Stdio::null()).spawn().map_err(|e| format!("ssh: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped");
    stdin.write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    // Closed: the script is read whole, and sh runs it.
    drop(stdin);
    Ok(child)
}

/// A script's output on the phone written to `path` as it comes, its sha256
/// worked out on the way; its size and hash.
pub fn download(host: &str, script: &str, path: &std::path::Path) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut child = spawn(host, script, Stdio::piped())?;
    let mut out = child.stdout.take().expect("piped");
    let mut file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = out.read(&mut buf).map_err(|e| format!("ssh: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", path.display()))?;
        size += n as u64;
    }
    file.sync_all().map_err(|e| format!("{}: {e}", path.display()))?;
    let status = child.wait().map_err(|e| format!("ssh: {e}"))?;
    if !status.success() {
        return Err(format!("reading on the phone failed ({status})"));
    }
    Ok((size, format!("{:x}", hash.finalize())))
}

/// A vetted script of itemgrid-core's own run past the guard - the parking
/// brake written into misc, the reboot into the bootloader. Never what is
/// typed: only the RAM boot's and flashing's own steps call this.
pub(crate) fn run_vetted(host: &str, script: &str) -> Result<String, String> {
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    if !out.status.success() {
        return Err(format!("on the phone: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A local file sent to the phone (`remote`, on its rootfs), and its sha256
/// there compared with the one here.
pub(crate) fn upload(host: &str, local: &std::path::Path, remote: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(local).map_err(|e| format!("{}: {e}", local.display()))?;
    let want = format!("{:x}", Sha256::digest(&bytes));
    // The file on standard input; the command line names only where it goes.
    let mut child = ssh(host, 4)
        .arg(format!("mkdir -p \"$(dirname '{remote}')\" && cat > '{remote}' && sync"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(&bytes).map_err(|e| format!("ssh: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    if !out.status.success() {
        return Err(format!("sending {}: {}", local.display(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    let there = run(host, &format!("sha256sum '{remote}' | cut -d' ' -f1\n"))?;
    if there.trim() != want {
        return Err(format!("{} arrived different on the phone", local.display()));
    }
    Ok(())
}

/// A phone asleep (item sends it to sleep when it is left): ssh times out,
/// though the USB link still answers a ping - a packet wakes it for a moment.
/// Pings and a fresh ssh in turn, up to ~40 s, until it answers.
pub fn wake(host: &str) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(40) {
        let _ = std::process::Command::new("ping").args(["-c", "3", "-i", "0.2", "-W", "1", host]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        if ssh_with(host, 6, false).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
            return true;
        }
    }
    false
}

/// Whether the link to `host` answers a ping (the phone is there, maybe asleep).
pub fn pings(host: &str) -> bool {
    if host == crate::link::CABLE && !crate::link::usb_up() {
        return false;
    }
    std::process::Command::new("ping").args(["-c", "1", "-W", "1", host]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The phone kept from sleeping while item/grid works on it (a kernel wakelock,
/// "itemgrid"), or let go. A reboot lets it go by itself.
pub fn keep_awake(host: &str, on: bool) -> Result<(), String> {
    let file = if on { "wake_lock" } else { "wake_unlock" };
    run(host, &format!("echo itemgrid > /sys/power/{file}\n")).map(|_| ())
}
