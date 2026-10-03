//! The phone over ssh: scripts are sent on the shell's standard input
//! (`sh -s`), never on its command line - a `pkill -f` pattern on the command
//! line matches the remote shell itself and kills it.

use std::io::Write;
use std::process::{Command, Stdio};

/// The addresses tried, in order: the USB network first, then any listed in
/// ~/.config/cradle/hosts (one per line, e.g. the phone's Wi-Fi address).
pub fn hosts() -> Vec<String> {
    let mut hosts = vec!["172.16.42.1".to_owned()];
    let path = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".config/cradle/hosts");
    if let Ok(text) = std::fs::read_to_string(path) {
        hosts.extend(text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned));
    }
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
    // the owner's known_hosts.
    c.args(["-o", "BatchMode=yes", "-o", &format!("ConnectTimeout={connect_timeout}"), "-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"]);
    if shared {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        c.args(["-o", "ControlMaster=auto", "-o", &format!("ControlPath={dir}/cradle-%C"), "-o", "ControlPersist=60", "-o", "ServerAliveInterval=2", "-o", "ServerAliveCountMax=3"]);
    } else {
        c.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    }
    c.arg(format!("root@{host}"));
    c
}

/// Whether the phone answers ssh at `host`.
pub fn answers(host: &str) -> bool {
    ssh(host, 2).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The same, over a new connection: while the phone reboots, a kept one may
/// not know yet that it is dead.
pub fn answers_fresh(host: &str) -> bool {
    ssh_with(host, 2, false).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The kept connection to `host` closed (before a reboot).
pub fn close_shared(host: &str) {
    let _ = ssh(host, 2).args(["-O", "exit"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// A script run on the phone as root; its standard output.
pub fn run(host: &str, script: &str) -> Result<String, String> {
    run_bytes(host, script).map(|b| String::from_utf8_lossy(&b).into_owned())
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
        "item did not start within 2 minutes - `cradle logs` shows why",
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
pub fn as_owner(script: &str) -> String {
    let user = r#"U=$(loginctl list-users --no-legend 2>/dev/null | awk '$2 != "root" {print $2; exit}'); U=${U:-droidian}; I=$(id -u "$U")"#;
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
    crate::guard::check(script)?;
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(out).stderr(Stdio::null()).spawn().map_err(|e| format!("ssh: {e}"))?;
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

/// A vetted script of cradle-core's own run past the guard - the parking
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
