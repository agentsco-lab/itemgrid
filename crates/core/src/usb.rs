//! What is on the USB, without a driver's help: the phone by its serial
//! number (stock Android starting, Android without debugging), the port's
//! kernel with no system (Halium's initramfs, 18d1:d001), the port's Linux
//! itself (its network gadget, 1d6b:0104), and whether the phone's USB
//! network is up on this computer. On Linux from /sys and the kernel's own
//! route table; on Windows by enumerating the devices (nusb) and the
//! adapters' addresses (if-addrs).

/// Halium's initramfs on the USB: the kernel up with nothing to start.
pub const INITRAMFS: (u16, u16) = (0x18d1, 0xd001);
/// The port's Linux on the USB: its RNDIS gadget (adaptation/access/sfduo-usb-gadget.sh).
pub const LINUX_GADGET: (u16, u16) = (0x1d6b, 0x0104);

/// A USB device with this serial number attached. Not by vendor id: other
/// Microsoft hardware (a Kinect, say) shares 045e.
pub fn serial_present(serial: &str) -> bool {
    #[cfg(unix)]
    {
        std::fs::read_dir("/sys/bus/usb/devices").is_ok_and(|d| d.flatten().any(|e| std::fs::read_to_string(e.path().join("serial")).is_ok_and(|s| s.trim() == serial)))
    }
    #[cfg(windows)]
    {
        devices().any(|d| d.serial_number() == Some(serial))
    }
}

/// A USB device with this vendor and product id attached.
pub fn present(id: (u16, u16)) -> bool {
    #[cfg(unix)]
    {
        std::fs::read_dir("/sys/bus/usb/devices").is_ok_and(|d| {
            d.flatten().any(|e| {
                let read = |f: &str| std::fs::read_to_string(e.path().join(f)).map(|s| s.trim().to_owned()).unwrap_or_default();
                read("idVendor") == format!("{:04x}", id.0) && read("idProduct") == format!("{:04x}", id.1)
            })
        })
    }
    #[cfg(windows)]
    {
        devices().any(|d| (d.vendor_id(), d.product_id()) == id)
    }
}

#[cfg(windows)]
fn devices() -> impl Iterator<Item = nusb::DeviceInfo> {
    use nusb::MaybeFuture;
    nusb::list_devices().wait().map(|l| l.collect::<Vec<_>>()).unwrap_or_default().into_iter()
}

/// Whether the phone's USB network is up on this computer: an interface
/// holding an address in 172.16.42.0/24 (other than the phone's own .1).
/// Without it 172.16.42.1 is no phone - some other network can answer there
/// (one did, 8 ms away), and the cable's ssh checks no host key.
pub fn network_up() -> bool {
    #[cfg(unix)]
    {
        // Read from the kernel's own table (/proc/net/fib_trie: each local
        // address a line, "/32 host LOCAL" under it) - `ip addr show` was run
        // for it, a process six times a second from the window's main
        // thread, in the frames (2026-10-08).
        std::fs::read_to_string("/proc/net/fib_trie").is_ok_and(|trie| network_in(&trie))
    }
    #[cfg(windows)]
    {
        if_addrs::get_if_addrs().is_ok_and(|all| all.iter().any(|i| ours(&i.ip().to_string())))
    }
}

/// Whether `address` is this computer's end of the phone's USB network.
fn ours(address: &str) -> bool {
    address.starts_with("172.16.42.") && address != "172.16.42.1"
}

#[cfg_attr(not(unix), allow(dead_code))]
fn network_in(trie: &str) -> bool {
    let mut address: Option<&str> = None;
    for line in trie.lines() {
        let t = line.trim_start_matches([' ', '|', '+', '-']).trim_end();
        if t.starts_with("/32 host LOCAL") {
            if address.is_some_and(ours) {
                return true;
            }
        } else if t.starts_with(|c: char| c.is_ascii_digit()) {
            address = Some(t);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIE: &str = "Main:
  +-- 0.0.0.0/0 3 0 5
     |-- 0.0.0.0
        /0 universe UNICAST
     +-- 172.16.42.0/24 2 0 2
        |-- 172.16.42.0
           /24 link UNICAST
        |-- 172.16.42.2
           /32 host LOCAL
Local:
  +-- 127.0.0.0/8 2 0 2
     |-- 127.0.0.1
        /32 host LOCAL
";

    #[test]
    fn the_usb_network_is_seen_by_its_own_address() {
        assert!(network_in(TRIE));
        // The phone's own address on this side is no phone's network.
        assert!(!network_in(&TRIE.replace("172.16.42.2", "172.16.42.1")));
        assert!(!network_in(&TRIE.replace("172.16.42", "172.16.43")));
        assert!(!network_in(""));
    }
}
