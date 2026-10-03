//! The safety rules for what is sent to the phone. Every script goes through
//! `check` before it leaves the computer.

/// What is never sent, and why.
const REFUSED: &[(&str, &str)] = &[
    ("/sys/kernel/debug/gpio", "reading the GPIO debug file resets the Duo at once"),
];

/// Whether a script may be sent to the phone; the reason if not.
pub fn check(script: &str) -> Result<(), String> {
    for (pattern, why) in REFUSED {
        if script.contains(pattern) {
            return Err(format!("refused: {why} ({pattern})"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_gpio_debug() {
        assert!(super::check("cat /sys/kernel/debug/gpio").is_err());
        assert!(super::check("uptime").is_ok());
    }
}
