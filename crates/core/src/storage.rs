//! What the phone's system disk holds, by part - as Finder's storage bar
//! shows an iPhone's. Counting takes a few seconds (du), so it is asked for
//! when the phone comes and after a change, not at every look.

/// The parts of the rootfs, in KiB: System is what the others leave of the
/// used space.
#[derive(Debug, Clone, Default)]
pub struct Parts {
    pub size: u64,
    pub free: u64,
    pub home: u64,
    pub logs: u64,
    pub caches: u64,
}

impl Parts {
    pub fn system(&self) -> u64 {
        (self.size - self.free).saturating_sub(self.home + self.logs + self.caches)
    }

    /// (name, KiB) of each part, the free space last.
    pub fn list(&self) -> [(&'static str, u64); 5] {
        [("System", self.system()), ("Home", self.home), ("Logs", self.logs), ("Caches", self.caches), ("Free", self.free)]
    }
}

const SCRIPT: &str = r#"
df -Pk / | awk 'NR==2 {print "size=" $2; print "free=" $4}'
for p in home:/home logs:/var/log caches:/var/cache; do
  echo "${p%%:*}=$(du -sxk "${p#*:}" 2>/dev/null | cut -f1)"
done
"#;

/// Counts the parts on the phone at `host`.
pub fn read(host: &str) -> Result<Parts, String> {
    let text = crate::phone::run(host, SCRIPT)?;
    let mut p = Parts::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v: u64 = v.trim().parse().unwrap_or(0);
        match k {
            "size" => p.size = v,
            "free" => p.free = v,
            "home" => p.home = v,
            "logs" => p.logs = v,
            "caches" => p.caches = v,
            _ => {}
        }
    }
    if p.size == 0 {
        return Err("the disk's size was not read".into());
    }
    Ok(p)
}
