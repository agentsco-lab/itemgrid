//! The computer's Wi-Fi given to the phone: the network this computer is
//! on (NetworkManager), its password read here and written on the phone as
//! a NetworkManager connection of its own (root's, 0600), so a freshly
//! installed Duo joins it by itself - the setup's Wi-Fi step finds it
//! joined (the owner, 2026-10-25: "the computer shares its Wi-Fi password,
//! as if it brought the phone into the network").
//!
//! The password goes in the script on ssh's standard input, never on a
//! command line here or there, and is never printed or logged. WPA/WPA3
//! personal and open networks; an enterprise one (802.1X) is left alone.

use std::process::Command;

/// The computer's network, as the phone is to have it.
pub struct Network {
    pub ssid: String,
    /// The key management (wpa-psk, sae) and its password; none: open.
    secret: Option<(String, String)>,
}

impl Network {
    pub fn secured(&self) -> bool {
        self.secret.is_some()
    }
}

fn nmcli(args: &[&str]) -> Option<String> {
    let out = Command::new("nmcli").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned())
}

/// The Wi-Fi network this computer is on now, with its password (nmcli
/// gives the user's own connections' secrets to the user).
pub fn computer() -> Result<Network, String> {
    let active = nmcli(&["-t", "-f", "NAME,TYPE", "connection", "show", "--active"]).ok_or("nmcli: no NetworkManager on this computer")?;
    let name = active
        .lines()
        .filter_map(|l| l.rsplit_once(':'))
        .find(|(_, t)| *t == "802-11-wireless")
        .map(|(n, _)| n.replace("\\:", ":").replace("\\\\", "\\"))
        .ok_or("this computer is not on Wi-Fi")?;
    let get = |field: &str, secrets: bool| {
        let mut args = vec!["-g", field, "connection", "show", "id", name.as_str()];
        if secrets {
            args.insert(0, "-s");
        }
        nmcli(&args).unwrap_or_default()
    };
    let ssid = get("802-11-wireless.ssid", false);
    if ssid.is_empty() {
        return Err("the computer's Wi-Fi has no name".into());
    }
    let mgmt = get("802-11-wireless-security.key-mgmt", false);
    let secret = match mgmt.as_str() {
        "" | "none" => None,
        "wpa-psk" | "sae" => {
            let psk = get("802-11-wireless-security.psk", true);
            if psk.is_empty() {
                return Err("the computer did not give its Wi-Fi password (kept for another user, or asked each time)".into());
            }
            Some((mgmt, psk))
        }
        other => return Err(format!("the computer's Wi-Fi is {other}: not given to the phone")),
    };
    Ok(Network { ssid, secret })
}

/// A value as a key file has it (GKeyFile's escapes).
fn keyfile_value(v: &str) -> String {
    let mut out = String::new();
    for (i, c) in v.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ' ' if i == 0 => out.push_str("\\s"),
            c => out.push(c),
        }
    }
    out
}

/// The script that writes `net` on the phone, joins it, and says so (the
/// password inside it: never echoed).
fn script(net: &Network) -> String {
    let id = format!("item-{}", net.ssid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(24).collect::<String>());
    let mut file = format!("[connection]\nid={}\ntype=wifi\nautoconnect=true\n\n[wifi]\nmode=infrastructure\nssid={}\n\n", keyfile_value(&net.ssid), keyfile_value(&net.ssid));
    if let Some((mgmt, psk)) = &net.secret {
        file.push_str(&format!("[wifi-security]\nkey-mgmt={mgmt}\npsk={}\n\n", keyfile_value(psk)));
    }
    file.push_str("[ipv4]\nmethod=auto\n\n[ipv6]\nmethod=auto\n");
    // The heredoc's end: a word the file cannot hold on a line of its own.
    let end = "ITEMGRID_WIFI_END_7f3a";
    format!(
        "set -e\nset +x\ndir=/etc/NetworkManager/system-connections\nf=\"$dir/{id}.nmconnection\"\nfor c in $(nmcli -t -f NAME,TYPE connection show | awk -F: '$2==\"802-11-wireless\"{{print $1}}'); do\n  [ \"$(nmcli -g 802-11-wireless.ssid connection show id \"$c\" 2>/dev/null)\" = {ssid_q} ] && {{ echo known; exit 0; }}\ndone\numask 077\ncat > \"$f\" <<'{end}'\n{file}{end}\nchmod 600 \"$f\"\nnmcli connection reload\nnmcli --wait 30 connection up filename \"$f\" >/dev/null 2>&1 && echo joined || echo kept\n",
        ssid_q = shell_quote(&net.ssid),
    )
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// What the phone said: joined now, kept to join when in reach, or known
/// already.
#[derive(Debug, PartialEq)]
pub enum Shared {
    Joined,
    Kept,
    Known,
}

/// The computer's network written on the phone at `host` (root's ssh).
pub fn share(host: &str) -> Result<(String, Shared), String> {
    let net = computer()?;
    let said = crate::phone::run_checked(host, &script(&net)).map_err(|_| "the phone did not take the network".to_owned())?;
    let shared = match said.lines().last().unwrap_or("") {
        "joined" => Shared::Joined,
        "known" => Shared::Known,
        _ => Shared::Kept,
    };
    Ok((net.ssid, shared))
}

/// The computer's time zone, set on the phone (a fresh one is on UTC): the
/// zone's name only, checked against the phone's own list.
pub fn share_zone(host: &str) -> Result<String, String> {
    let zone = Command::new("timedatectl").args(["show", "-p", "Timezone", "--value"]).output().map_err(|e| e.to_string())?;
    let zone = String::from_utf8_lossy(&zone.stdout).trim().to_owned();
    if zone.is_empty() || !zone.chars().all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c)) {
        return Err("the computer's time zone is not known".into());
    }
    crate::phone::run_checked(host, &format!("[ -f /usr/share/zoneinfo/{zone} ] && timedatectl set-timezone {zone}"))?;
    Ok(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyfile_escapes() {
        assert_eq!(keyfile_value(" a\\b\nc"), "\\sa\\\\b\\nc");
        assert_eq!(keyfile_value("plain;text"), "plain;text");
    }

    #[test]
    fn script_keeps_the_secret_off_command_lines() {
        let net = Network { ssid: "Home's net".into(), secret: Some(("wpa-psk".into(), "s3cr3t pass".into())) };
        let s = script(&net);
        // The password only inside the heredoc.
        let in_heredoc = s.split("<<'ITEMGRID_WIFI_END_7f3a'").nth(1).unwrap();
        assert!(in_heredoc.contains("psk=s3cr3t pass"));
        assert!(!s.split("<<'ITEMGRID_WIFI_END_7f3a'").next().unwrap().contains("s3cr3t"));
        assert!(s.contains("'Home'\\''s net'"));
    }
}
