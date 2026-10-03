//! cradle-core: what a connected Surface Duo is doing, and the safe ways to
//! act on it. The command line (crates/cli) and, later, the window are only
//! ways of showing what is here: every action and every safety rule lives in
//! this crate, so neither can be gone around.

pub mod detect;
pub mod guard;
pub mod phone;
pub mod status;

pub use detect::{detect, Mode, Seen};
