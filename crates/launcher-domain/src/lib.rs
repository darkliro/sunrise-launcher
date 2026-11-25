//! The launcher's pure types: what a catalogue release is, how game builds order, and the dates a
//! Nightly's version carries. Nothing here touches the filesystem, the network, a process or a
//! window, so the CLI, the MCP server, the UI and tests share it without the launcher's cost.
mod channel;
mod game_tags;
pub mod nightly;
mod release;

pub use channel::Channel;
pub use game_tags::{compare_game_versions, dev_date, is_game_tag, stable_version};
pub use nightly::date_of;
pub use release::Release;
