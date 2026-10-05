//! The battery in detail, for Hythe's Battery section: what the power
//! supply says (health, cycles, capacity, voltage, current, temperature) and
//! the charge over the last days from UPower's history.

/// The battery as the phone tells it.
#[derive(Debug, Clone, Default)]
pub struct Battery {
    pub percent: Option<u32>,
    /// The kernel's word: Charging, Discharging, Full, Not charging.
    pub state: String,
    pub health: String,
    pub cycles: Option<u32>,
    /// mAh it holds now, and when new.
    pub full_mah: Option<u32>,
    pub design_mah: Option<u32>,
    pub volts: Option<f64>,
    /// mA as the kernel says it (this one: negative while it charges).
    pub current_ma: Option<f64>,
    pub temp_c: Option<f64>,
    /// The charge over time: unix seconds, percent, UPower's state word;
    /// oldest first, the last two days.
    pub history: Vec<(i64, f64, String)>,
}

const HISTORY_S: i64 = 48 * 3600;

/// Reads the battery on the phone at `host`.
pub fn read(host: &str) -> Result<Battery, String> {
    // UPower's charge history, all its files (one per id it gave the
    // battery, a new one now and then).
    let script = r#"cd /sys/class/power_supply/battery 2>/dev/null && for f in capacity status health cycle_count charge_full charge_full_design voltage_now current_now temp; do printf '%s=%s\n' $f "$(cat $f 2>/dev/null)"; done
echo --
for f in /var/lib/upower/history-charge-*.dat; do case $f in *generic*) ;; *) tail -n 3000 "$f";; esac; done
"#;
    let out = crate::phone::run(host, script)?;
    let (facts, history) = out.split_once("--\n").unwrap_or((&out, ""));
    let get = |k: &str| facts.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(str::trim).unwrap_or_default().to_owned();
    let num = |k: &str| get(k).parse::<f64>().ok();
    let now = history.lines().filter_map(|l| l.split_whitespace().next()?.parse::<i64>().ok()).max().unwrap_or(0);
    // "0.000 unknown" is UPower's line for a start before the battery was
    // read: not a charge.
    let mut history: Vec<(i64, f64, String)> = history
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let t = w.next()?.parse::<i64>().ok()?;
            let p = w.next()?.parse::<f64>().ok()?;
            Some((t, p, w.next().unwrap_or_default().to_owned()))
        })
        .filter(|(t, _, s)| *t > now - HISTORY_S && s != "unknown")
        .collect();
    history.sort_by_key(|h| h.0);
    history.dedup_by_key(|h| h.0);
    Ok(Battery {
        percent: num("capacity").map(|v| v as u32),
        state: get("status"),
        health: get("health"),
        cycles: num("cycle_count").map(|v| v as u32),
        full_mah: num("charge_full").map(|v| (v / 1000.0) as u32),
        design_mah: num("charge_full_design").map(|v| (v / 1000.0) as u32),
        volts: num("voltage_now").map(|v| v / 1e6),
        current_ma: num("current_now").map(|v| v / 1000.0),
        temp_c: num("temp").map(|v| v / 10.0),
        history,
    })
}
