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
/// gives the user's own connections' secrets to the user). Asked a few
/// times: at the end of an install, the computer busy with the image, the
/// password once did not come (2026-10-25).
pub fn computer() -> Result<Network, String> {
    let mut last = String::new();
    for i in 0..4 {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        match computer_once() {
            Ok(n) => return Ok(n),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn computer_once() -> Result<Network, String> {
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
    share_net(host, computer()?)
}

/// `net` (taken from the computer earlier: at an install's start, while
/// the computer is free) written on the phone.
pub fn share_net(host: &str, net: Network) -> Result<(String, Shared), String> {
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

/// A zone's main place and where it is, from zone1970.tab (its
/// coordinates as ±DDMM[SS]±DDDMM[SS]): its name, latitude and longitude
/// in degrees.
fn zone_place(tab: &str, zone: &str) -> Option<(String, f64, f64)> {
    let line = tab.lines().filter(|l| !l.starts_with('#')).find(|l| l.split('\t').nth(2) == Some(zone))?;
    let coords = line.split('\t').nth(1)?;
    let split = coords[1..].find(['+', '-'])? + 1;
    let deg = |s: &str, d: usize| -> Option<f64> {
        let sign = if s.starts_with('-') { -1.0 } else { 1.0 };
        let n = &s[1..];
        let whole: f64 = n.get(..d)?.parse().ok()?;
        let min: f64 = n.get(d..d + 2)?.parse().ok()?;
        let sec: f64 = n.get(d + 2..d + 4).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        Some(sign * (whole + min / 60.0 + sec / 3600.0))
    };
    let lat = deg(&coords[..split], 2)?;
    let lon = deg(&coords[split..], 3)?;
    let name = zone.rsplit('/').next()?.replace('_', " ");
    Some((name, lat, lon))
}

/// The weather's place on the phone (GNOME Weather's first city, which
/// item's weather reads), the zone's main city, unless it has one: a fresh
/// phone has none, and no weather.
pub fn share_weather_place(host: &str, zone: &str) -> Result<Option<String>, String> {
    let tab = std::fs::read_to_string("/usr/share/zoneinfo/zone1970.tab").map_err(|e| format!("zone1970.tab: {e}"))?;
    let (name, lat, lon) = zone_place(&tab, zone).ok_or("the zone has no place")?;
    if !name.chars().all(|c| c.is_alphanumeric() || " -".contains(c)) {
        return Err("the place's name is not plain".into());
    }
    let value = format!("[<(uint32 2, <('{name}', '', true, [({:.6}, {:.6})], @a(dd) [])>)>]", lat.to_radians(), lon.to_radians());
    // Right after the install's restart the user's session bus may not be
    // up yet, and gsettings then keeps the value in memory and loses it
    // (2026-10-25): its socket waited for, dconf asked for by name (an
    // error, not memory, without it), and the value read back.
    let script = format!(
        "for i in $(seq 60); do [ -S \"$XDG_RUNTIME_DIR/bus\" ] && break; sleep 0.5; done\nexport GSETTINGS_BACKEND=dconf\n[ \"$(gsettings get org.gnome.Weather locations)\" = '@av []' ] || {{ echo kept; exit 0; }}\ngsettings set org.gnome.Weather locations \"{value}\"\n[ \"$(gsettings get org.gnome.Weather locations)\" != '@av []' ] && echo set || echo lost"
    );
    let said = crate::phone::run_checked(host, &crate::phone::as_owner(&script))?;
    match said.lines().last().unwrap_or("") {
        "set" => Ok(Some(name)),
        "kept" => Ok(None),
        _ => Err("the phone did not keep the weather's place".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_places() {
        let tab = "# comment\nUA\t+5026+03031\tEurope/Kyiv\tmost of Ukraine\nUS\t+404251-0740023\tAmerica/New_York\tEastern\n";
        let (n, lat, lon) = zone_place(tab, "Europe/Kyiv").unwrap();
        assert_eq!(n, "Kyiv");
        assert!((lat - 50.4333).abs() < 0.001 && (lon - 30.5167).abs() < 0.001);
        let (n, lat, lon) = zone_place(tab, "America/New_York").unwrap();
        assert_eq!(n, "New York");
        assert!((lat - 40.7142).abs() < 0.001 && (lon + 74.0064).abs() < 0.001);
    }

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
