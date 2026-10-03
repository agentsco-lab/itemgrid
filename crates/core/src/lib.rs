//! cradle-core: what a connected Surface Duo is doing, and the safe ways to
//! act on it. The command line (crates/cli) and, later, the window are only
//! ways of showing what is here: every action and every safety rule lives in
//! this crate, so neither can be gone around.

pub mod backup;
pub mod detect;
pub mod guard;
pub mod live;
pub mod logs;
pub mod phone;
pub mod screenshot;
pub mod status;
pub mod storage;
pub mod update;

pub use detect::{detect, Mode, Seen};
