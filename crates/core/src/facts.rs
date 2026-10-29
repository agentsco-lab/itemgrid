//! What a Duo in stock Android says of itself over adb, as Android's shell
//! user may read it (no root): the build, the bootloader's lock, the data
//! partition's size and the partitions' names - Windows (the WOA
//! Project's `esp` and `win`) among them, if it is there. Read only.

/// A Duo in Android, as it says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub model: String,
    pub device: String,
    pub build: String,
    pub android: String,
    /// "1" locked (ro.boot.flash.locked), "0" unlocked.
    pub locked: String,
    /// green: locked, the stock system verified; orange: unlocked.
    pub boot_state: String,
    /// Settings' OEM unlocking (sys.oem_unlock_allowed): "1" allows
    /// `fastboot flashing unlock`.
    pub unlock_allowed: String,
    pub slot: String,
    /// /data's size, as df has it ("222G").
    pub data: String,
    /// The partitions of the main disk (sda) by name, in its order.
    pub main_disk: Vec<String>,
}

impl Facts {
    /// Windows beside Android: its partitions on the main disk.
    pub fn windows(&self) -> bool {
        self.main_disk.iter().any(|p| p == "win")
    }

    pub fn lines(&self) -> Vec<(&'static str, String)> {
        let lock = match (self.locked.as_str(), self.unlock_allowed.as_str()) {
            ("1", "1") => "locked; OEM unlocking allowed in Settings".to_owned(),
            ("1", _) => "locked; OEM unlocking off in Settings".to_owned(),
            ("0", _) => format!("unlocked ({})", self.boot_state),
            (other, _) => format!("unknown ({other})"),
        };
        let mut l = vec![
            ("Model", format!("{} ({})", self.model, self.device)),
            ("Android", format!("{}, build {}", self.android, self.build)),
            ("Lock", lock),
            ("Slot", self.slot.trim_start_matches('_').to_owned()),
            ("Data", self.data.clone()),
            ("Disk", self.main_disk.join(" ")),
        ];
        if self.windows() {
            l.push(("Windows", "on it beside Android (the WOA Project's esp and win)".into()));
        }
        l
    }
}

const SCRIPT: &str = "for p in ro.product.model ro.product.device ro.build.display.id ro.build.version.release ro.boot.flash.locked ro.boot.verifiedbootstate sys.oem_unlock_allowed ro.boot.slot_suffix; do echo \"$p=$(getprop $p)\"; done; \
echo \"data=$(df -h /data | tail -1 | awk '{print $2}')\"; \
ls -l /dev/block/by-name | awk '$NF ~ /^\\/dev\\/block\\/sda[0-9]+$/ {sub(\"/dev/block/sda\", \"\", $NF); print \"part=\" $NF \" \" $(NF-2)}'";

/// Read from the phone `serial` in Android.
pub fn read(serial: &str) -> Result<Facts, String> {
    parse(&crate::full::adb_shell(serial, SCRIPT)?)
}

fn parse(out: &str) -> Result<Facts, String> {
    let mut f = Facts::default();
    let mut parts: Vec<(u32, String)> = Vec::new();
    for line in out.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let v = v.trim().to_owned();
        match k {
            "ro.product.model" => f.model = v,
            "ro.product.device" => f.device = v,
            "ro.build.display.id" => f.build = v,
            "ro.build.version.release" => f.android = v,
            "ro.boot.flash.locked" => f.locked = v,
            "ro.boot.verifiedbootstate" => f.boot_state = v,
            "sys.oem_unlock_allowed" => f.unlock_allowed = v,
            "ro.boot.slot_suffix" => f.slot = v,
            "data" => f.data = v,
            "part" => {
                if let Some((n, name)) = v.split_once(' ').and_then(|(n, name)| Some((n.parse().ok()?, name.to_owned()))) {
                    parts.push((n, name));
                }
            }
            _ => {}
        }
    }
    if f.device.is_empty() {
        return Err("Android did not say what it is".into());
    }
    parts.sort();
    f.main_disk = parts.into_iter().map(|(_, n)| n).collect();
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duo_2_as_it_said() {
        // The owner's Duo 2, 2026-10-29 (stock, 256 GB).
        let out = "ro.product.model=Surface Duo 2\nro.product.device=duo2\nro.build.display.id=2023.501.541\nro.build.version.release=12\nro.boot.flash.locked=1\nro.boot.verifiedbootstate=green\nsys.oem_unlock_allowed=0\nro.boot.slot_suffix=_a\ndata=222G\npart=4 frp\npart=3 metadata\npart=5 misc\npart=2 persist\npart=6 rawdump\npart=1 ssd\npart=8 userdata\npart=7 vm-data\n";
        let f = parse(out).unwrap();
        assert_eq!(f.main_disk, ["ssd", "persist", "metadata", "frp", "misc", "rawdump", "vm-data", "userdata"]);
        assert!(!f.windows());
        assert_eq!(f.lines()[2].1, "locked; OEM unlocking off in Settings");
        let woa = out.replace("part=8 userdata", "part=8 esp\npart=9 win\npart=10 userdata");
        assert!(parse(&woa).unwrap().windows());
    }
}
