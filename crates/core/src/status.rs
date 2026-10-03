//! The phone's state when Linux is up: one script, its answer as
//! `key=value` lines.

use std::collections::HashMap;

const SCRIPT: &str = r#"
v() { dpkg-query -W -f='${Version}' "$1" 2>/dev/null; }
echo "os=$(. /etc/os-release; echo "$PRETTY_NAME")"
echo "kernel=$(uname -r)"
echo "uptime=$(cut -d. -f1 /proc/uptime)"
echo "item=$(v item-shell)"
echo "item_built=$(date -r /usr/libexec/item/item-compositor '+%F %R' 2>/dev/null)"
echo "item_running=$(systemctl is-active item.service 2>/dev/null)"
echo "port=$(v adaptation-droidian-surfaceduo)"
echo "sensorfw=$(v sensorfw-qt6)"
b=/sys/class/power_supply/battery
echo "battery=$(cat $b/capacity 2>/dev/null)"
echo "battery_status=$(cat $b/status 2>/dev/null)"
echo "battery_temp=$(cat $b/temp 2>/dev/null)"
t=0
for z in /sys/class/thermal/thermal_zone*; do
  case "$(cat $z/type 2>/dev/null)" in cpu*|gpu*) x=$(cat $z/temp 2>/dev/null) && [ "${x:-0}" -gt "$t" ] && t=$x;; esac
done
echo "cpu_temp=$t"
df -P / /userdata 2>/dev/null | awk 'NR>1 {print "disk " $6 " " $2 " " $4}'
systemctl --failed --no-legend --plain 2>/dev/null | awk '{print "failed=" $1}'
"#;

/// The phone's state, read once.
#[derive(Debug, Default)]
pub struct Status {
    pub os: String,
    pub kernel: String,
    pub uptime_s: u64,
    pub item: String,
    pub item_built: String,
    pub item_running: bool,
    pub port: String,
    pub sensorfw: String,
    pub battery: Option<u32>,
    pub battery_status: String,
    /// Degrees Celsius.
    pub battery_temp: Option<f64>,
    pub cpu_temp: Option<f64>,
    /// Mount point, size and free space (KiB).
    pub disks: Vec<(String, u64, u64)>,
    pub failed: Vec<String>,
}

impl Status {
    /// Things worth a look, in words.
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        if !self.item_running {
            w.push("item is not running".to_owned());
        }
        for (mount, size, free) in &self.disks {
            if *size > 0 && (*free as f64) < *size as f64 * 0.05 {
                w.push(format!("{mount} is nearly full: {} free", size_words(*free)));
            }
        }
        if self.battery.is_some_and(|b| b <= 15) && self.battery_status != "Charging" {
            w.push("the battery is low".to_owned());
        }
        if self.cpu_temp.is_some_and(|t| t >= 70.0) {
            w.push("the CPU is hot".to_owned());
        }
        w
    }
}

/// Reads the phone's state over ssh at `host`.
pub fn read(host: &str) -> Result<Status, String> {
    let text = crate::phone::run(host, SCRIPT)?;
    let mut kv: HashMap<&str, &str> = HashMap::new();
    let mut s = Status::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("disk ") {
            let mut p = rest.split_whitespace();
            if let (Some(m), Some(size), Some(free)) = (p.next(), p.next(), p.next()) {
                s.disks.push((m.to_owned(), size.parse().unwrap_or(0), free.parse().unwrap_or(0)));
            }
        } else if let Some(unit) = line.strip_prefix("failed=") {
            s.failed.push(unit.to_owned());
        } else if let Some((k, v)) = line.split_once('=') {
            kv.insert(k, v);
        }
    }
    let get = |k: &str| kv.get(k).copied().unwrap_or_default().to_owned();
    s.os = get("os");
    s.kernel = get("kernel");
    s.uptime_s = get("uptime").parse().unwrap_or(0);
    s.item = get("item");
    s.item_built = get("item_built");
    s.item_running = get("item_running") == "active";
    s.port = get("port");
    s.sensorfw = get("sensorfw");
    s.battery = get("battery").parse().ok();
    s.battery_status = get("battery_status");
    s.battery_temp = get("battery_temp").parse::<f64>().ok().map(|t| t / 10.0);
    s.cpu_temp = get("cpu_temp").parse::<f64>().ok().filter(|t| *t > 0.0).map(|t| t / 1000.0);
    Ok(s)
}

/// KiB in words: "2.1 GB".
pub fn size_words(kib: u64) -> String {
    let gb = kib as f64 / 1024.0 / 1024.0;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", kib as f64 / 1024.0) }
}
