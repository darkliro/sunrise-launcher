//! The launcher's pure types: what a catalogue release is, how game builds order, the dates a
//! Nightly's version carries, the channel a build belongs to, and the choices saved for an
//! instance. Nothing here touches the filesystem, the network, a process or a window, so the CLI,
//! the MCP server, the UI and tests share it without the launcher's cost.
mod channel;
mod game_tags;
mod icon;
mod launch;
mod meta;
pub mod nightly;
mod profile;
mod release;

pub use channel::Channel;
pub use game_tags::{compare_game_versions, dev_date, is_game_tag, stable_version};
pub use icon::{is_texture_name, Icon};
pub use launch::Launch;
pub use meta::Meta;
pub use nightly::date_of;
pub use profile::Profile;
pub use release::Release;
