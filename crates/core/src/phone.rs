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
    let mut c = Command::new("ssh");
    // The phone's host key changes with each image: it is not written into
    // the owner's known_hosts.
    c.args(["-o", "BatchMode=yes", "-o", &format!("ConnectTimeout={connect_timeout}"), "-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"]);
    c.arg(format!("root@{host}"));
    c
}

/// Whether the phone answers ssh at `host`.
pub fn answers(host: &str) -> bool {
    ssh(host, 2).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// A script run on the phone as root; its standard output.
pub fn run(host: &str, script: &str) -> Result<String, String> {
    crate::guard::check(script)?;
    let mut child = ssh(host, 4).args(["sh", "-s"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ssh: {e}"))?;
    child.stdin.take().expect("piped").write_all(script.as_bytes()).map_err(|e| format!("ssh: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    if out.status.code() == Some(255) {
        return Err(format!("ssh: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
