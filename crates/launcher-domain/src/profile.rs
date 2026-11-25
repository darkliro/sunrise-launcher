//! The native build setting an instance keeps. There is one build today, but the saved value is
//! a name so an unknown one can be ignored without losing the rest of the instance.
use serde::{Deserialize, Serialize};

/// Native mods use the SDK's optimized compilation settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    Optimized,
}

impl Profile {
    pub fn name(self) -> &'static str {
        "optimized"
    }

    pub fn cargo(self) -> (&'static str, &'static [&'static str]) {
        ("release", &[])
    }
}

#[cfg(test)]
mod tests {
    use super::Profile;

    #[test]
    fn the_saved_name_is_the_one_cargo_uses() {
        assert_eq!(Profile::default().name(), "optimized");
        let (cargo, features) = Profile::default().cargo();
        assert_eq!(cargo, "release");
        assert!(features.is_empty());
    }
}
