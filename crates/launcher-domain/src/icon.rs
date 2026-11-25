//! An instance's icon: a prepared block or item texture, or a colour.
use serde::{Deserialize, Serialize};

/// What an instance shows in the sidebar: a block or item texture from the player's prepared
/// files (never shipped), or a colour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icon {
    /// A texture name: `grass_block` (block/ is looked in first, then item/).
    Texture(String),
    /// `#rrggbb`.
    Colour(String),
}

impl Icon {
    /// `#3fa7d6` is a colour; a texture's name is lowercase letters, digits and `_`.
    pub fn parse(text: &str) -> Result<Icon, String> {
        let text = text.trim();
        if let Some(hex) = text.strip_prefix('#') {
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(Icon::Colour(format!("#{}", hex.to_ascii_lowercase())));
            }
        } else if is_texture_name(text) {
            return Ok(Icon::Texture(text.into()));
        }
        Err(format!(
            "{text}: give a block or item name (grass_block) or a colour (#3fa7d6)"
        ))
    }
}

/// A texture's name: lowercase letters, digits and `_`, so never a path.
pub fn is_texture_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::Icon;

    #[test]
    fn an_icon_is_a_texture_name_or_a_lowercase_colour() {
        assert_eq!(Icon::parse("#3FA7D6"), Ok(Icon::Colour("#3fa7d6".into())));
        assert_eq!(
            Icon::parse(" grass_block "),
            Ok(Icon::Texture("grass_block".into()))
        );
        for bad in ["", "#3fa7d", "#gggggg", "Grass", "a/b", "#"] {
            assert!(Icon::parse(bad).is_err(), "{bad}");
        }
    }
}
