//! The publication channel an instance follows, and the pure rules that order and classify builds.
//!
//! Anything that needs to know whether a tag or a release is Stable, Nightly or Dev, or which
//! published builds a channel offers, lives here. The two questions that read the launcher's
//! draft test-build mark stay in `instances`, because that mark is launcher state, not a pure type.

use crate::game_tags::{dev_date, is_game_tag};
use crate::nightly::date_of;
use crate::Release;
use serde::{Deserialize, Serialize};

/// Which builds an instance's "latest" follows. Missing in a saved instance: Nightly, so nobody
/// is moved to an older build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    #[default]
    Nightly,
    Dev,
}

impl Channel {
    /// Publication channels, not a classifier for arbitrary tags the player has on disk.
    pub fn of_release(release: &Release) -> Option<Self> {
        use sunrise_launcher_release_verify::{Channel as SignedChannel, NIGHTLY};
        match release.nightly.as_deref() {
            Some(NIGHTLY) => Some(Self::Nightly),
            Some(tag)
                if release.prerelease && SignedChannel::of(tag) == Some(SignedChannel::Dev) =>
            {
                Some(Self::Dev)
            }
            None if !release.prerelease && is_game_tag(&release.tag) => Some(Self::Stable),
            _ => None,
        }
    }

    /// Installed builds can be classified even if their releases have been pruned.
    pub fn of_build(tag: &str) -> Option<Self> {
        if date_of(tag).is_some() {
            Some(Self::Nightly)
        } else if dev_date(tag).is_some() {
            Some(Self::Dev)
        } else if is_game_tag(tag) {
            Some(Self::Stable)
        } else {
            None
        }
    }

    /// The downloadable releases on this channel, in catalogue order (newest first).
    pub fn builds(self, releases: &[Release]) -> impl Iterator<Item = &Release> {
        releases
            .iter()
            .filter(move |r| r.asset.is_some() && Self::of_release(r) == Some(self))
    }

    /// What an unpinned profile follows for now. Without a Stable download it plays Nightly
    /// without changing the saved channel; Stable takes over when one becomes available.
    pub fn following(self, releases: &[Release]) -> Self {
        if self == Self::Stable && self.builds(releases).next().is_none() {
            Self::Nightly
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;
    use crate::Release;

    fn release(tag: &str, prerelease: bool, nightly: Option<&str>) -> Release {
        Release {
            tag: tag.to_owned(),
            name: String::new(),
            body: String::new(),
            date: String::new(),
            time: String::new(),
            published_at: String::new(),
            asset: Some(("asset".to_owned(), 1)),
            prerelease,
            nightly: nightly.map(str::to_owned),
        }
    }

    #[test]
    fn classifies_tags_and_releases_by_channel() {
        assert_eq!(Channel::of_build("26.3-0.1.0"), Some(Channel::Stable));
        assert_eq!(
            Channel::of_build("nightly-2026-11-20-0c02067"),
            Some(Channel::Nightly)
        );
        assert_eq!(
            Channel::of_build("dev-2026-11-20-0c02067"),
            Some(Channel::Dev)
        );
        assert_eq!(Channel::of_build("26.3-0.1.0-rc1"), None);
        assert_eq!(
            Channel::of_release(&release("26.3-0.1.0", false, None)),
            Some(Channel::Stable)
        );
        assert_eq!(
            Channel::of_release(&release("nightly", false, Some("nightly"))),
            Some(Channel::Nightly)
        );
    }

    #[test]
    fn stable_falls_back_to_nightly_without_a_download() {
        let stable = release("26.3-0.1.0", false, None);
        assert_eq!(Channel::Stable.following(&[]), Channel::Nightly);
        assert_eq!(
            Channel::Stable.following(std::slice::from_ref(&stable)),
            Channel::Stable
        );
    }
}
