//! itemgrid-core: what a connected Surface Duo is doing, and the safe ways to
//! act on it. The command line (crates/cli) and, later, the window are only
//! ways of showing what is here: every action and every safety rule lives in
//! this crate, so neither can be gone around.

pub mod activity;
pub mod agent;
pub mod android;
pub mod backup;
pub mod battery;
pub mod bootchain;
pub mod clock;
pub mod club;
pub mod detect;
pub mod flash;
pub mod full;
pub mod guard;
pub mod install;
pub mod link;
pub mod live;
pub mod logs;
pub mod look;
pub mod moved;
pub mod paths;
pub mod phone;
pub mod posture;
pub mod programs;
pub mod ramboot;
pub mod restore;
pub mod screenshot;
pub mod slots;
pub mod status;
pub mod stock;
pub mod storage;
pub mod update;
pub mod usb;

pub use detect::{detect, Mode, Seen};
