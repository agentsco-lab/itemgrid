//! The programs item/grid runs on this computer - adb, fastboot, ssh - and
//! how: found next to item/grid (ITEMGRID_TOOLS, or the `tools` folder
//! beside the running program: the Windows installer brings adb and
//! fastboot there) or on the PATH; run with a limit, so a tool that hangs
//! (adb's server, stuck after the cable came out) holds nothing for long.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Where tools brought along are looked for.
fn tools_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ITEMGRID_TOOLS") {
        return Some(PathBuf::from(d));
    }
    let beside = std::env::current_exe().ok()?.parent()?.join("tools");
    beside.is_dir().then_some(beside)
}

/// A program by name: the one brought along if there is one, else the
/// system's (by PATH).
pub fn find(name: &str) -> PathBuf {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    match tools_dir().map(|d| d.join(&file)) {
        Some(p) if p.exists() => p,
        _ => PathBuf::from(name),
    }
}

pub fn adb() -> PathBuf {
    find("adb")
}

pub fn fastboot() -> PathBuf {
    find("fastboot")
}

/// Whether `name` can be run from here.
pub fn present(name: &str) -> bool {
    Command::new(find(name)).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

/// A command run with `limit`: its output, or None if it is missing, fails
/// to start or takes longer (then it is killed and reaped).
pub fn output_within(mut cmd: Command, limit: Duration) -> Option<Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let start = Instant::now();
    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether the process `pid` is still running.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: plain Win32 calls; the handle is closed before returning.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE as u32
        }
    }
}

/// Free room (bytes) on the volume holding `dir`.
pub fn free_space(dir: &std::path::Path) -> u64 {
    #[cfg(unix)]
    {
        Command::new("df")
            .args(["-Pk", &dir.display().to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().nth(1).and_then(|l| l.split_whitespace().nth(3).and_then(|v| v.parse::<u64>().ok())))
            .map(|k| k * 1024)
            .unwrap_or(0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut free: u64 = 0;
        // SAFETY: a valid, NUL-terminated path; the out pointers live for the call.
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) } != 0;
        if ok {
            free
        } else {
            0
        }
    }
}
