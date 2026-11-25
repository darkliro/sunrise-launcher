//! What runs around the game for one instance: a command before it, a wrapper, environment
//! variables, and a command after it exits. Only the shape lives here; building and running the
//! commands stays in the launcher (`launch`).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// A shell command run in the instance's folder before the game; the game starts only if it
    /// succeeds.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_launch: String,
    /// A command line the game runs through, such as `nice -n 5` (quotes group a word).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wrapper: String,
    /// A shell command run in the instance's folder after the game exits, whatever its exit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub post_exit: String,
    /// Environment variables for the game (and the two commands above).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Linux: run the game through Feral's GameMode (`gamemoderun`), when it is installed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub gamemode: bool,
    /// Linux: MangoHud's overlay (`mangohud`), when it is installed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mangohud: bool,
    /// Linux: the dedicated graphics card of a computer with two (PRIME render offload).
    #[serde(default, skip_serializing_if = "is_false")]
    pub dedicated_gpu: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Launch {
    pub fn is_empty(&self) -> bool {
        self == &Launch::default()
    }
}
