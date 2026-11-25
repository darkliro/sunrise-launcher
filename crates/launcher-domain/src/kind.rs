//! What an instance is: the game's home, one the launcher made, or a modpack from the registry.

/// How an instance was made, which says where its folder lives and what may change it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The game's home: what every player had before instances.
    Default,
    /// Made in the launcher: `instances/<slug>/`.
    Own,
    /// A modpack from Sunrise's registry.
    Pack,
}
