//! fastboot, spoken by item/grid itself over USB (nusb), in place of
//! Google's platform-tools binary: the bootloader's protocol is small - a
//! command as text in one bulk transfer, answers of INFO, TEXT, DATA, OKAY
//! or FAIL in one transfer each, and a download sent after DATA.
//!
//! `run` takes the command lines item/grid gives fastboot (devices, getvar,
//! oem, flashing unlock, erase, flash, boot, continue, reboot) and answers
//! in fastboot's own words - "(bootloader) ..." for INFO, "OKAY" and
//! "FAILED (remote: '...')" - so the checks on its output (an OKAY, a
//! bootloader's warning signs) read it as they read fastboot's.
//!
//! `ITEMGRID_FASTBOOT=platform` sends the commands to platform-tools'
//! fastboot instead (to compare the two).

use std::time::{Duration, Instant};

use nusb::transfer::{Bulk, In, Out};
use nusb::MaybeFuture;

/// fastboot's interface: vendor class, subclass 0x42, protocol 3.
const CLASS: (u8, u8, u8) = (0xff, 0x42, 0x03);
/// A download's piece, one transfer.
const CHUNK: usize = 1 << 20;
/// An answer's most bytes.
const ANSWER: usize = 256;

/// Whether the platform's fastboot is to be used instead.
pub fn platform() -> bool {
    std::env::var("ITEMGRID_FASTBOOT").is_ok_and(|v| v == "platform")
}

/// The phones in fastboot on USB: their serials.
pub fn devices() -> Vec<String> {
    let Ok(list) = nusb::list_devices().wait() else { return Vec::new() };
    list.filter(|d| d.interfaces().any(|i| (i.class(), i.subclass(), i.protocol()) == CLASS)).map(|d| d.serial_number().unwrap_or("????????").to_owned()).collect()
}

struct Device {
    _iface: nusb::Interface,
    ep_in: nusb::Endpoint<Bulk, In>,
    ep_out: nusb::Endpoint<Bulk, Out>,
    max_packet: usize,
}

/// What a command was answered.
enum Answer {
    Okay(String),
    Fail(String),
    Data(usize),
}

impl Device {
    fn open(serial: Option<&str>) -> Result<Device, String> {
        let list = nusb::list_devices().wait().map_err(|e| format!("USB: {e}"))?;
        let mut found = list.filter(|d| d.interfaces().any(|i| (i.class(), i.subclass(), i.protocol()) == CLASS)).filter(|d| serial.is_none_or(|s| d.serial_number() == Some(s)));
        let info = found.next().ok_or("< waiting for any device >: no phone in fastboot")?;
        if serial.is_none() && found.next().is_some() {
            return Err("more than one phone in fastboot: say which (-s SERIAL)".into());
        }
        let number = info.interfaces().find(|i| (i.class(), i.subclass(), i.protocol()) == CLASS).map(|i| i.interface_number()).ok_or("no fastboot interface")?;
        let device = info.open().wait().map_err(|e| format!("USB: {e} (the udev rule for the phone?)"))?;
        let config = device.active_configuration().map_err(|e| format!("USB: {e}"))?;
        let (mut ep_in_addr, mut ep_out_addr, mut max_packet) = (None, None, 512);
        for alt in config.interface_alt_settings().filter(|a| a.interface_number() == number) {
            for ep in alt.endpoints() {
                if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                    continue;
                }
                max_packet = ep.max_packet_size();
                match ep.direction() {
                    nusb::transfer::Direction::In => ep_in_addr = Some(ep.address()),
                    nusb::transfer::Direction::Out => ep_out_addr = Some(ep.address()),
                }
            }
        }
        let (Some(a_in), Some(a_out)) = (ep_in_addr, ep_out_addr) else { return Err("the fastboot interface has no bulk endpoints".into()) };
        let iface = device.detach_and_claim_interface(number).wait().map_err(|e| format!("USB: {e}"))?;
        let ep_in = iface.endpoint::<Bulk, In>(a_in).map_err(|e| format!("USB: {e}"))?;
        let ep_out = iface.endpoint::<Bulk, Out>(a_out).map_err(|e| format!("USB: {e}"))?;
        Ok(Device { _iface: iface, ep_in, ep_out, max_packet })
    }

    fn send(&mut self, data: &[u8], timeout: Duration) -> Result<(), String> {
        let c = self.ep_out.transfer_blocking(data.to_vec().into(), timeout);
        c.status.map_err(|e| format!("USB: {e}"))?;
        if c.actual_len != data.len() {
            return Err(format!("USB: {} of {} bytes sent", c.actual_len, data.len()));
        }
        Ok(())
    }

    /// One answer; its INFO and TEXT lines into `said` until the final one.
    fn answer(&mut self, timeout: Duration, said: &mut String) -> Result<Answer, String> {
        let until = Instant::now() + timeout;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("the bootloader did not answer in time".into());
            }
            let len = ANSWER.div_ceil(self.max_packet) * self.max_packet;
            let c = self.ep_in.transfer_blocking(self.ep_in.allocate(len), left);
            match c.status {
                Ok(()) => {}
                Err(nusb::transfer::TransferError::Cancelled) => return Err("the bootloader did not answer in time".into()),
                Err(e) => return Err(format!("USB: {e}")),
            }
            let bytes = &c.buffer[..c.actual_len];
            if bytes.len() < 4 {
                continue;
            }
            let (kind, rest) = bytes.split_at(4);
            let text = String::from_utf8_lossy(rest).trim_end_matches('\0').to_owned();
            match kind {
                b"INFO" => {
                    said.push_str("(bootloader) ");
                    said.push_str(&text);
                    said.push('\n');
                }
                b"TEXT" => said.push_str(&text),
                b"OKAY" => return Ok(Answer::Okay(text)),
                b"FAIL" => return Ok(Answer::Fail(text)),
                b"DATA" => return Ok(Answer::Data(usize::from_str_radix(text.trim(), 16).map_err(|_| format!("DATA '{text}'"))?)),
                other => return Err(format!("an answer the bootloader should not give: {:?}", String::from_utf8_lossy(other))),
            }
        }
    }

    /// A command, answered.
    fn command(&mut self, cmd: &str, timeout: Duration, said: &mut String) -> Result<Answer, String> {
        if cmd.len() > 64 {
            return Err(format!("command longer than fastboot's 64 bytes: {cmd}"));
        }
        self.send(cmd.as_bytes(), Duration::from_secs(5))?;
        self.answer(timeout, said)
    }

    /// `data` downloaded into the bootloader's memory.
    fn download(&mut self, data: &[u8], timeout: Duration, said: &mut String) -> Result<Answer, String> {
        match self.command(&format!("download:{:08x}", data.len()), Duration::from_secs(10), said)? {
            Answer::Data(n) if n == data.len() => {}
            Answer::Data(n) => return Err(format!("the bootloader asked for {n} bytes, not {}", data.len())),
            other => return Ok(other),
        }
        let start = Instant::now();
        for piece in data.chunks(CHUNK) {
            let left = timeout.saturating_sub(start.elapsed()).max(Duration::from_secs(5));
            self.send(piece, left)?;
        }
        self.answer(Duration::from_secs(30), said)
    }
}

/// An answer in fastboot's words: OKAY, or FAILED with the bootloader's.
fn told(a: Answer, started: Instant, said: &mut String) -> bool {
    match a {
        Answer::Okay(_) => {
            said.push_str(&format!("OKAY [{:>7.3}s]\n", started.elapsed().as_secs_f64()));
            true
        }
        Answer::Fail(m) => {
            said.push_str(&format!("FAILED (remote: '{m}')\n"));
            false
        }
        Answer::Data(_) => {
            said.push_str("FAILED (the bootloader asked for data unasked)\n");
            false
        }
    }
}

/// fastboot's command line, run by item/grid itself; its output as
/// fastboot gives it (stdout and stderr together).
pub fn run(args: &[&str], limit: Duration) -> Result<String, String> {
    let (serial, rest) = match args {
        ["-s", s, rest @ ..] => (Some(*s), rest),
        rest => (None, rest),
    };
    if rest == ["devices"] {
        return Ok(devices().into_iter().filter(|s| serial.is_none_or(|w| w == s)).map(|s| format!("{s}\t fastboot\n")).collect());
    }
    let mut dev = Device::open(serial)?;
    let mut said = String::new();
    let started = Instant::now();
    let ok = match rest {
        ["getvar", "all"] => {
            let a = dev.command("getvar:all", limit, &mut said)?;
            said.push_str("all:\n");
            matches!(a, Answer::Okay(_))
        }
        ["getvar", var] => match dev.command(&format!("getvar:{var}"), limit, &mut said)? {
            Answer::Okay(v) => {
                said.push_str(&format!("{var}: {v}\n"));
                true
            }
            a => told(a, started, &mut said),
        },
        ["oem", words @ ..] => {
            let a = dev.command(&format!("oem {}", words.join(" ")), limit, &mut said)?;
            told(a, started, &mut said)
        }
        ["flashing", what] => {
            let a = dev.command(&format!("flashing {what}"), limit, &mut said)?;
            told(a, started, &mut said)
        }
        ["erase", part] => {
            said.push_str(&format!("Erasing '{part}' "));
            let a = dev.command(&format!("erase:{part}"), limit, &mut said)?;
            told(a, started, &mut said)
        }
        ["flash", part, file] => {
            let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            said.push_str(&format!("Sending '{part}' ({} KB) ", data.len() / 1024));
            let a = dev.download(&data, limit, &mut said)?;
            if told(a, started, &mut said) {
                said.push_str(&format!("Writing '{part}' "));
                let a = dev.command(&format!("flash:{part}"), limit, &mut said)?;
                told(a, Instant::now(), &mut said)
            } else {
                false
            }
        }
        ["boot", file] => {
            let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            said.push_str(&format!("Sending 'boot.img' ({} KB) ", data.len() / 1024));
            let a = dev.download(&data, limit, &mut said)?;
            if told(a, started, &mut said) {
                said.push_str("Booting ");
                let a = dev.command("boot", limit, &mut said)?;
                told(a, Instant::now(), &mut said)
            } else {
                false
            }
        }
        ["continue"] => {
            said.push_str("Resuming boot ");
            let a = dev.command("continue", limit, &mut said)?;
            told(a, started, &mut said)
        }
        ["reboot"] => {
            said.push_str("Rebooting ");
            let a = dev.command("reboot", limit, &mut said)?;
            told(a, started, &mut said)
        }
        ["reboot", "bootloader"] => {
            said.push_str("Rebooting into bootloader ");
            let a = dev.command("reboot-bootloader", limit, &mut said)?;
            told(a, started, &mut said)
        }
        other => return Err(format!("fastboot {}: not a command item/grid speaks", other.join(" "))),
    };
    said.push_str(&format!("Finished. Total time: {:.3}s\n", started.elapsed().as_secs_f64()));
    let _ = ok;
    Ok(said)
}
