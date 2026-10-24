//! PID 1: the filesystems a shell needs, the partitions by name, the USB
//! gadget's skeleton, and the adb daemon kept running as a child; every
//! orphan reaped.

use std::ffi::CString;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::say;

/// Where item/grid looks for the partitions (android.rs's BLK), and the
/// names Android's ueventd gives them too.
const BY_NAME: &str = "/dev/block/platform/soc/1d84000.ufshc/by-name";
/// The UFS controller, as its block devices' sysfs paths name it.
const UFS: &str = "1d84000.ufshc";
pub const GADGET: &str = "/sys/kernel/config/usb_gadget/g1";
pub const FFS: &str = "/dev/usb-ffs/adb";

pub fn run() -> ! {
    mounts();
    say!("up ({})", env!("CARGO_PKG_VERSION"));
    // The screen on its own thread: drawn as the status changes.
    let serial = serial();
    std::thread::spawn(move || crate::screen::run(serial));
    by_name();
    if let Err(e) = gadget() {
        say!("the USB gadget: {e}");
    }
    let _ = std::process::Command::new("/bin/busybox").args(["ip", "link", "set", "lo", "up"]).status();
    supervise();
}

fn mount(src: &str, target: &str, fs: &str, flags: libc::c_ulong, data: &str) {
    let _ = std::fs::create_dir_all(target);
    let (s, t, f, d) = (CString::new(src).unwrap(), CString::new(target).unwrap(), CString::new(fs).unwrap(), CString::new(data).unwrap());
    // SAFETY: valid C strings for the call's duration.
    let r = unsafe { libc::mount(s.as_ptr(), t.as_ptr(), f.as_ptr(), flags, d.as_ptr().cast()) };
    if r != 0 {
        say!("mount {target}: {}", std::io::Error::last_os_error());
    }
}

fn mounts() {
    mount("proc", "/proc", "proc", libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC, "");
    mount("sysfs", "/sys", "sysfs", libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC, "");
    mount("devtmpfs", "/dev", "devtmpfs", libc::MS_NOSUID, "mode=0755");
    mount("devpts", "/dev/pts", "devpts", libc::MS_NOSUID | libc::MS_NOEXEC, "gid=5,mode=0620");
    mount("tmpfs", "/tmp", "tmpfs", libc::MS_NOSUID | libc::MS_NODEV, "mode=1777");
    mount("tmpfs", "/run", "tmpfs", libc::MS_NOSUID | libc::MS_NODEV, "mode=0755");
    mount("configfs", "/sys/kernel/config", "configfs", 0, "");
    std::env::set_var("PATH", "/sbin:/bin");
    std::env::set_var("HOME", "/tmp");
}

/// The UFS partitions as nodes in /dev/block and linked by their names, as
/// Android's ueventd does:
/// waited for (the controller probes after init starts) until userdata is
/// there, 20 s at most.
fn by_name() {
    let start = Instant::now();
    loop {
        let n = link_partitions();
        if Path::new(&format!("{BY_NAME}/userdata")).exists() {
            say!("{n} partitions by name");
            break;
        }
        if start.elapsed() > Duration::from_secs(20) {
            say!("no userdata among {n} partitions after 20 s");
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    for alias in ["/dev/block/bootdevice", "/dev/block/by-name"] {
        let _ = std::fs::remove_file(alias);
    }
    let _ = std::os::unix::fs::symlink("/dev/block/platform/soc/1d84000.ufshc", "/dev/block/bootdevice");
    let _ = std::os::unix::fs::symlink(BY_NAME, "/dev/block/by-name");
}

fn link_partitions() -> usize {
    let _ = std::fs::create_dir_all(BY_NAME);
    let Ok(dir) = std::fs::read_dir("/sys/class/block") else { return 0 };
    let mut n = 0;
    for e in dir.flatten() {
        let real = std::fs::canonicalize(e.path()).unwrap_or_default();
        if !real.to_string_lossy().contains(UFS) {
            continue;
        }
        let Ok(uevent) = std::fs::read_to_string(e.path().join("uevent")) else { continue };
        let get = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k)).map(str::to_owned);
        let (Some(name), Some(dev)) = (get("PARTNAME="), get("DEVNAME=")) else { continue };
        // The node where Android has it, /dev/block/sdXN (item/grid checks
        // a partition resolves there before it writes), its name a link to
        // it - as ueventd makes them.
        let node = format!("/dev/block/{dev}");
        if !Path::new(&node).exists() {
            let (Some(major), Some(minor)) = (get("MAJOR=").and_then(|v| v.parse::<u32>().ok()), get("MINOR=").and_then(|v| v.parse::<u32>().ok())) else { continue };
            let c = CString::new(node.as_str()).unwrap_or_default();
            // SAFETY: mknod with a valid path for a block device.
            if unsafe { libc::mknod(c.as_ptr(), libc::S_IFBLK | 0o600, libc::makedev(major, minor)) } != 0 {
                continue;
            }
        }
        let link = format!("{BY_NAME}/{name}");
        if !Path::new(&link).exists() {
            let _ = std::os::unix::fs::symlink(&node, &link);
        }
        n += 1;
    }
    n
}

/// The phone's serial as the bootloader passes it (the one fastboot and
/// Android give, so item/grid's `adb -s` finds it).
pub fn serial() -> String {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    cmdline.split_whitespace().find_map(|w| w.strip_prefix("androidboot.serialno=")).unwrap_or("item-recovery").to_owned()
}

fn put(path: &str, value: &str) -> Result<(), String> {
    std::fs::write(path, value).map_err(|e| format!("{path}: {e}"))
}

/// The gadget with one function, adb over FunctionFS; bound to the
/// controller by the daemon once it has written the descriptors.
fn gadget() -> Result<(), String> {
    let mk = |p: &str| std::fs::create_dir_all(p).map_err(|e| format!("{p}: {e}"));
    mk(GADGET)?;
    put(&format!("{GADGET}/idVendor"), "0x18d1")?;
    put(&format!("{GADGET}/idProduct"), "0xd001")?;
    put(&format!("{GADGET}/bcdDevice"), "0x0100")?;
    put(&format!("{GADGET}/bcdUSB"), "0x0200")?;
    mk(&format!("{GADGET}/strings/0x409"))?;
    put(&format!("{GADGET}/strings/0x409/serialnumber"), &serial())?;
    put(&format!("{GADGET}/strings/0x409/manufacturer"), "Microsoft")?;
    put(&format!("{GADGET}/strings/0x409/product"), "Surface Duo (item recovery)")?;
    mk(&format!("{GADGET}/configs/b.1/strings/0x409"))?;
    put(&format!("{GADGET}/configs/b.1/strings/0x409/configuration"), "adb")?;
    put(&format!("{GADGET}/configs/b.1/MaxPower"), "500")?;
    mk(&format!("{GADGET}/functions/ffs.adb"))?;
    let link = format!("{GADGET}/configs/b.1/f1");
    if !Path::new(&link).exists() {
        std::os::unix::fs::symlink(format!("{GADGET}/functions/ffs.adb"), &link).map_err(|e| format!("{link}: {e}"))?;
    }
    mount("adb", FFS, "functionfs", 0, "");
    Ok(())
}

/// The controller the gadget binds to (a600000.dwc3).
pub fn udc() -> Option<String> {
    std::fs::read_dir("/sys/class/udc").ok()?.flatten().next().map(|e| e.file_name().to_string_lossy().into_owned())
}

/// The daemon run and run again; every child that ends reaped (orphans of
/// the shell's pipelines come to PID 1).
fn supervise() -> ! {
    let me = std::env::current_exe().unwrap_or_else(|_| "/sbin/item-recovery".into());
    loop {
        let child = std::process::Command::new(&me).arg("adbd").spawn();
        let pid = match child {
            Ok(c) => c.id() as libc::pid_t,
            Err(e) => {
                say!("the daemon did not start: {e}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        loop {
            let mut status = 0;
            // SAFETY: waitpid with a valid pointer.
            let r = unsafe { libc::waitpid(-1, &mut status, 0) };
            if r == pid {
                say!("the daemon ended ({status}): started again");
                std::thread::sleep(Duration::from_millis(500));
                break;
            }
            if r < 0 {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn sync() {
    // SAFETY: sync takes nothing.
    unsafe { libc::sync() };
}

/// Restarted, into what `to` names (bootloader, recovery) or the system.
pub fn reboot(to: Option<&str>) -> ! {
    sync();
    say!("reboot {}", to.unwrap_or(""));
    match to.filter(|t| !t.is_empty()) {
        Some(t) => {
            let arg = CString::new(t).unwrap_or_default();
            // SAFETY: the reboot syscall with its magic numbers and a C string.
            unsafe {
                libc::syscall(libc::SYS_reboot, libc::LINUX_REBOOT_MAGIC1, libc::LINUX_REBOOT_MAGIC2, libc::LINUX_REBOOT_CMD_RESTART2, arg.as_ptr());
            }
        }
        // SAFETY: reboot takes a command.
        None => unsafe {
            libc::reboot(libc::RB_AUTOBOOT);
        },
    }
    std::process::exit(1)
}

pub fn power_off() -> ! {
    sync();
    // SAFETY: reboot takes a command.
    unsafe { libc::reboot(libc::RB_POWER_OFF) };
    std::process::exit(1)
}
