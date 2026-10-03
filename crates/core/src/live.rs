//! The phone's screen live, from item's mirror (item-compositor's
//! mirror.rs): frames as they are drawn, a quarter of the size, read through
//! ssh as the owner - the mirror's socket is theirs. On the phone a few
//! lines of python carry the socket to ssh (it has no socat or nc).

use std::io::Read;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};

const RELAY: &str = r#"exec python3 -u -c "
import os, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(os.environ['XDG_RUNTIME_DIR'] + '/item-mirror.sock')
out = sys.stdout.buffer
while True:
    data = s.recv(1 << 20)
    if not data:
        break
    out.write(data)
    out.flush()
""#;

/// The output's panels at the mirror's size: x ranges, the hinge's strip
/// between them left out (1350 and 1434 of 2784, over four).
const PANELS: [(usize, usize); 2] = [(0, 337), (359, 696)];

/// A frame: the two panels, each the right way up (RGBA).
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub panels: [Vec<u8>; 2],
}

/// The live view: frames read one after another; stopped from anywhere.
pub struct Live {
    out: ChildStdout,
    child: Arc<Mutex<Child>>,
}

/// Stops a live view from another thread.
#[derive(Clone)]
pub struct Stop(Arc<Mutex<Child>>);

impl Stop {
    /// Whether two stops stop the same view.
    pub fn same(&self, other: &Stop) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn stop(&self) {
        let _ = self.0.lock().map(|mut c| c.kill());
    }
}

impl Live {
    pub fn start(host: &str) -> Result<(Live, Stop), String> {
        let mut child = crate::phone::spawn(host, &crate::phone::as_owner(RELAY), Stdio::piped())?;
        let out = child.stdout.take().ok_or("no output")?;
        let child = Arc::new(Mutex::new(child));
        Ok((Live { out, child: child.clone() }, Stop(child)))
    }

    /// The next frame; an error when the view ends (stopped, item gone, or
    /// no mirror in this item).
    pub fn next(&mut self) -> Result<Frame, String> {
        let mut head = [0u8; 20];
        self.out.read_exact(&mut head).map_err(|_| "the live view ended (an item without a mirror?)".to_owned())?;
        if &head[..4] != b"IMF1" {
            return Err("not item's mirror".into());
        }
        let n = |i: usize| u32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]) as usize;
        let (w, h, raw, packed) = (n(4), n(8), n(12), n(16));
        let mut buf = vec![0u8; packed];
        self.out.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let rgba = lz4_flex::block::decompress(&buf, raw).map_err(|e| e.to_string())?;
        if rgba.len() != w * h * 4 || w < PANELS[1].1 {
            return Err(format!("a frame of {w}x{h} does not fit"));
        }
        let panels = PANELS.map(|(a, b)| {
            let mut out = Vec::with_capacity((b - a) * h * 4);
            for y in (0..h).rev() {
                out.extend_from_slice(&rgba[(y * w + a) * 4..(y * w + b) * 4]);
            }
            out
        });
        Ok(Frame { width: PANELS[0].1 - PANELS[0].0, height: h, panels })
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.child.lock().map(|mut c| {
            let _ = c.kill();
            let _ = c.wait();
        });
    }
}
