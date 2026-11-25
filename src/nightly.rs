//! The rolling Nightly channel, alongside Stable. The game repository publishes it as `nightly`;
//! each build installs under its own `nightly-<date>-<commit>` name after version-bound signature
//! verification. Updating that channel is explicit; an exact profile pin remains exact.
use crate::backend::{self, Release};
use std::{fs, path::Path};
pub use sunrise_launcher_domain::date_of;
use sunrise_launcher_domain::nightly::is_date;
use sunrise_launcher_release_verify::{is_nightly, NIGHTLY};

/// The version a nightly installs as: `nightly-2026-10-28-0c02067`, from its title
/// ("Nightly 2026-10-28 · 0c02067"), else its publication date.
pub fn version_of(release: &Release) -> String {
    let mut words = release
        .name
        .strip_prefix("Nightly ")
        .unwrap_or_default()
        .split(" · ");
    let date = words.next().filter(|d| is_date(d)).unwrap_or(&release.date);
    let commit = words
        .next()
        .filter(|c| !c.is_empty() && c.chars().all(|c| c.is_ascii_hexdigit()));
    match commit {
        Some(commit) => format!("{NIGHTLY}-{date}-{commit}"),
        None => format!("{NIGHTLY}-{date}"),
    }
}

/// The localized name of a dated Nightly, such as "Nightly · Nov 20".
pub fn display_name(tag: &str) -> Option<String> {
    let date = date_of(tag)?;
    Some(crate::i18n::trf(
        "{channel} · {version}",
        &[
            ("channel", crate::i18n::tr("Nightly")),
            ("version", &day(date)),
        ],
    ))
}

/// A day as players read it, "Nov 1", from `2026-11-01`; anything else as it is.
pub fn day(date: &str) -> String {
    use crate::i18n::trf;
    let day = date
        .get(8..10)
        .map(|d| d.trim_start_matches('0').to_string())
        .unwrap_or_default();
    let args = [("day", day.as_str())];
    match date.get(5..7) {
        Some("01") => trf("Jan {day}", &args),
        Some("02") => trf("Feb {day}", &args),
        Some("03") => trf("Mar {day}", &args),
        Some("04") => trf("Apr {day}", &args),
        Some("05") => trf("May {day}", &args),
        Some("06") => trf("Jun {day}", &args),
        Some("07") => trf("Jul {day}", &args),
        Some("08") => trf("Aug {day}", &args),
        Some("09") => trf("Sep {day}", &args),
        Some("10") => trf("Oct {day}", &args),
        Some("11") => trf("Nov {day}", &args),
        Some("12") => trf("Dec {day}", &args),
        _ => date.to_string(),
    }
}

/// The Nightlies of the last release list read that share their day with another, and the time
/// (`HH:MM` UTC) each came out: [`same_day_time`] reads it for every screen that names a version.
static SAME_DAY: std::sync::RwLock<Vec<(String, String)>> = std::sync::RwLock::new(Vec::new());
/// Full RFC 3339 publication timestamps seen for rolling Nightlies, kept even after the rolling
/// tag rotates to a newer build so same-day updates compare full timestamps.
static PUBLISHED: std::sync::RwLock<Vec<(String, String)>> = std::sync::RwLock::new(Vec::new());

const CURRENT_INSTALL: &str = ".nightly-current";
const FAILED_INSTALL_RECEIPT: &str = ".nightly-receipt-failed";

/// Record the verified rolling build, independently of the global or profile selection.
/// A concurrent reader sees the previous choice or the complete new one, never a partial tag.
pub(crate) fn record_current(versions: &Path, tag: &str) -> std::io::Result<()> {
    write_receipt(versions, CURRENT_INSTALL, tag)?;
    let _ = fs::remove_file(versions.join(FAILED_INSTALL_RECEIPT));
    Ok(())
}

pub(crate) fn has_current_receipt(versions: &Path, tag: &str) -> bool {
    receipt_matches(&versions.join(CURRENT_INSTALL), tag)
}

/// The record a live retry saw. A later successful record must not be overwritten by a stale notice.
pub(crate) fn current_receipt(versions: &Path) -> Option<String> {
    use std::io::Read as _;
    use sunrise_launcher_release_verify::{is_build, Channel};
    let path = versions.join(CURRENT_INSTALL);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || crate::native_cache::linked(&meta) {
        return None;
    }
    let mut tag = String::new();
    // A full bound tag (prefix, ISO date and up to 40 hex digits) is at most 59 bytes.
    fs::File::open(path)
        .ok()?
        .take(60)
        .read_to_string(&mut tag)
        .ok()?;
    is_build(Channel::Nightly, &tag).then_some(tag)
}

pub(crate) fn failed_install_receipt(versions: &Path, tag: &str) -> bool {
    receipt_matches(&versions.join(FAILED_INSTALL_RECEIPT), tag)
}

fn receipt_matches(path: &Path, tag: &str) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| {
        meta.is_file() && !crate::native_cache::linked(&meta) && meta.len() == tag.len() as u64
    }) && fs::read(path).is_ok_and(|bytes| bytes == tag.as_bytes())
}

pub(crate) fn remember_install_receipt_failure(versions: &Path, tag: &str) {
    // The error may also prevent this independent write (e.g. a full volume). A
    // missing note never blocks using the image or implies an interrupted install.
    let _ = write_receipt(versions, FAILED_INSTALL_RECEIPT, tag);
}

fn write_receipt(versions: &Path, name: &str, tag: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut part = tempfile::NamedTempFile::new_in(versions)?;
    part.write_all(tag.as_bytes())?;
    part.persist(versions.join(name))?;
    Ok(())
}

/// The newest installed Nightly we can identify without assuming that commit hashes are ordered.
/// The saved rolling install breaks a tie on the newest date. Older installs may instead prove the
/// order with a full set of publication times and a unique latest time; otherwise keep a saved choice.
pub(crate) fn known_install<'a>(
    versions: &Path,
    installed: &'a [String],
    selected: Option<&str>,
) -> Option<&'a String> {
    let mut nightlies = installed
        .iter()
        .filter_map(|tag| Some((date_of(tag)?, tag)));
    let (newest_date, first) = nightlies.next()?;
    let mut same_day = nightlies.take_while(|(date, _)| *date == newest_date);
    let Some((_, second)) = same_day.next() else {
        return Some(first);
    };
    let candidates: Vec<_> = [first, second]
        .into_iter()
        .chain(same_day.map(|(_, tag)| tag))
        .collect();
    if let Ok(current) = fs::read_to_string(versions.join(CURRENT_INSTALL)) {
        if let Some(tag) = candidates.iter().copied().find(|tag| **tag == current) {
            return Some(tag);
        }
    }
    let mut stamped: Vec<_> = candidates
        .iter()
        .map(|tag| {
            let published =
                fs::read_to_string(versions.join(tag).join(INSTALLED_PUBLISHED)).ok()?;
            Some((
                chrono::DateTime::parse_from_rfc3339(published.trim()).ok()?,
                *tag,
            ))
        })
        .collect::<Option<_>>()
        .unwrap_or_default();
    stamped.sort_unstable_by_key(|(time, _)| *time);
    if let [.., (prior, _), (latest, tag)] = stamped.as_slice() {
        if latest > prior {
            return Some(tag);
        }
    }
    let mut timed: Vec<_> = candidates
        .iter()
        .map(|tag| {
            let time = fs::read_to_string(versions.join(tag).join(INSTALLED_TIME)).ok()?;
            Some((
                chrono::NaiveTime::parse_from_str(time.trim(), "%H:%M").ok()?,
                *tag,
            ))
        })
        .collect::<Option<_>>()
        .unwrap_or_default();
    timed.sort_unstable_by_key(|(time, _)| *time);
    if let [.., (prior, _), (latest, tag)] = timed.as_slice() {
        if latest > prior {
            return Some(tag);
        }
    }
    candidates
        .into_iter()
        .find(|tag| Some(tag.as_str()) == selected)
}

/// The publication time kept beside an installed Nightly so it survives a launcher restart.
pub(crate) const INSTALLED_TIME: &str = ".nightly-time";
/// The full RFC 3339 publication timestamp kept beside an installed Nightly.
pub(crate) const INSTALLED_PUBLISHED: &str = ".nightly-published";

/// Keeps the publication time that came with an installed Nightly.
pub(crate) fn record_install_time(dir: &Path, release: &Release) -> std::io::Result<()> {
    if release.nightly.as_deref().is_some_and(is_nightly) {
        if !release.time.is_empty() {
            fs::write(dir.join(INSTALLED_TIME), &release.time)?;
        }
        if !release.published_at.is_empty() {
            fs::write(dir.join(INSTALLED_PUBLISHED), &release.published_at)?;
        }
    }
    Ok(())
}

/// Publication times saved beside installed Nightlies, including ones no longer in the release
/// list after a restart.
fn installed_times() -> Vec<(String, String)> {
    installed_field(INSTALLED_TIME)
}

fn installed_published() -> Vec<(String, String)> {
    installed_field(INSTALLED_PUBLISHED)
}

fn installed_field(file: &str) -> Vec<(String, String)> {
    backend::installed()
        .into_iter()
        .filter_map(|tag| {
            date_of(&tag)?;
            let value = fs::read_to_string(backend::version_dir(&tag).join(file))
                .ok()?
                .trim()
                .to_owned();
            (!value.is_empty()).then_some((tag, value))
        })
        .collect()
}

/// Keeps, from the release list `releases`, when each Nightly that shares its day with another
/// came out, so every name of it says its time. A Nightly the list no longer has (older than a
/// week, say) keeps the time it was seen with, and still counts toward its day.
pub fn note_published(releases: &[Release]) {
    let installed = installed_times();
    let mut kept = SAME_DAY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *kept = same_day_times(&kept, releases, &installed);
    let installed_full = installed_published();
    let mut full = PUBLISHED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *full = merge_published(&full, releases, &installed_full);
}

/// A scoped table of full RFC 3339 publication timestamps seen for rolling Nightlies, preserving
/// earlier builds' timestamps when the rolling `nightly` tag rotates.
#[derive(Default)]
pub struct PublishedTimestamps(Vec<(String, String)>);

impl PublishedTimestamps {
    pub fn note(&mut self, releases: &[Release]) {
        self.0 = merge_published(&self.0, releases, &[]);
    }

    pub fn get(&self, version: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(tag, _)| tag == version)
            .map(|(_, published)| published.clone())
    }
}

fn merge_published(
    kept: &[(String, String)],
    releases: &[Release],
    installed: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = releases
        .iter()
        .filter(|r| r.nightly.as_deref().is_some_and(is_nightly) && !r.published_at.is_empty())
        .map(|r| (r.tag.clone(), r.published_at.clone()))
        .collect();
    for (tag, published) in kept.iter().chain(installed) {
        if !published.is_empty()
            && date_of(tag).is_some()
            && out.iter().all(|(existing, _)| existing != tag)
        {
            out.push((tag.clone(), published.clone()));
        }
    }
    out
}

/// Full RFC 3339 publication timestamp seen or saved for Nightly `version`, when known.
pub fn published_at(version: &str) -> Option<String> {
    if let Some(found) = PUBLISHED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(tag, _)| tag == version)
        .map(|(_, published)| published.clone())
    {
        return Some(found);
    }
    date_of(version)?;
    let saved = fs::read_to_string(backend::version_dir(version).join(INSTALLED_PUBLISHED))
        .ok()?
        .trim()
        .to_owned();
    (!saved.is_empty()).then_some(saved)
}

/// Rebuilds the same-day table from the current releases, process-local history, and times saved
/// with installed Nightlies.
fn same_day_times(
    kept: &[(String, String)],
    releases: &[Release],
    installed: &[(String, String)],
) -> Vec<(String, String)> {
    let mut known: Vec<(&str, &str, &str)> = releases
        .iter()
        .filter(|r| r.nightly.as_deref().is_some_and(is_nightly))
        .map(|r| (r.tag.as_str(), r.date.as_str(), r.time.as_str()))
        .collect();
    for (tag, time) in kept.iter().chain(installed) {
        if let Some(date) = date_of(tag).filter(|_| known.iter().all(|(t, ..)| t != tag)) {
            known.push((tag, date, time));
        }
    }
    known
        .iter()
        .filter(|(_, date, time)| {
            !time.is_empty() && known.iter().filter(|(_, d, _)| d == date).count() > 1
        })
        .map(|(tag, _, time)| (tag.to_string(), time.to_string()))
        .collect()
}

/// When Nightly `version` came out, `HH:MM` UTC, if another came out the same day.
pub fn same_day_time(version: &str) -> Option<String> {
    SAME_DAY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(tag, _)| tag == version)
        .map(|(_, time)| time.clone())
}

/// A Nightly described as an update, "the Nov 13 update" for `nightly-2026-11-13` (with
/// or without its commit), with its time when another came out that day ("the Nov 13, 07:22 UTC
/// update"); None for anything that is not one.
pub fn update_name(version: &str) -> Option<String> {
    let date = date_of(version)?;
    Some(match same_day_time(version) {
        Some(time) => crate::i18n::trf(
            "the {day}, {time} UTC update",
            &[("day", &day(date)), ("time", &time)],
        ),
        None => crate::i18n::trf("the {day} update", &[("day", &day(date))]),
    })
}

/// A full 40-character commit as a player reads it, `b1ff51d9e…`; anything else as it is.
pub fn short_commit(version: &str) -> String {
    if version.len() == 40 && version.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("{}…", &version[..9])
    } else {
        version.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(name: &str, date: &str) -> Release {
        Release {
            tag: NIGHTLY.into(),
            name: name.into(),
            body: String::new(),
            date: date.into(),
            time: String::new(),
            published_at: String::new(),
            asset: Some(("https://example/sunrise.zip".into(), 1)),
            prerelease: true,
            nightly: None,
        }
    }

    fn nightly_release(version: &str, date: &str, time: &str) -> Release {
        Release {
            tag: version.into(),
            time: time.into(),
            nightly: Some(NIGHTLY.into()),
            ..release("", date)
        }
    }

    #[test]
    fn a_nightly_installs_under_its_date_and_commit() {
        let r = release("Nightly 2026-10-28 · 0c02067", "2026-10-28");
        assert_eq!(version_of(&r), "nightly-2026-10-28-0c02067");
        let untitled = release("Something else", "2026-10-29");
        assert_eq!(version_of(&untitled), "nightly-2026-10-29");
        assert_eq!(date_of(&version_of(&r)), Some("2026-10-28"));
        assert_eq!(date_of("v0.2.95"), None);
        assert_eq!(date_of("nightly"), None);
        assert!(sunrise_launcher_release_verify::is_nightly(&version_of(&r)));
    }

    #[test]
    fn two_nightlies_of_one_day_are_named_with_their_times() {
        let nightly = |version: &str, date: &str, time: &str| Release {
            tag: version.into(),
            time: time.into(),
            nightly: Some(NIGHTLY.into()),
            ..release("", date)
        };
        note_published(&[
            nightly("nightly-2031-03-04-bbbbbbb", "2031-03-04", "18:05"),
            nightly("nightly-2031-03-04-aaaaaaa", "2031-03-04", "07:22"),
            nightly("nightly-2031-03-03-ccccccc", "2031-03-03", "09:00"),
        ]);
        let late = update_name("nightly-2031-03-04-bbbbbbb").unwrap();
        let early = update_name("nightly-2031-03-04-aaaaaaa").unwrap();
        assert_ne!(late, early);
        assert!(
            late.contains("18:05") && early.contains("07:22"),
            "{late} / {early}"
        );
        // Alone on its day, a Nightly keeps its day's name.
        assert_eq!(
            update_name("nightly-2031-03-03-ccccccc"),
            update_name("nightly-2031-03-03")
        );
    }

    #[test]
    fn a_nightly_the_list_no_longer_has_keeps_its_time() {
        let nightly = |version: &str, time: &str| Release {
            tag: version.into(),
            time: time.into(),
            nightly: Some(NIGHTLY.into()),
            ..release("", "2032-05-06")
        };
        let late = nightly("nightly-2032-05-06-bbbbbbb", "18:05");
        note_published(&[late.clone(), nightly("nightly-2032-05-06-aaaaaaa", "07:22")]);
        // A later list without the earlier build: both still read apart.
        note_published(&[late]);
        assert_eq!(
            same_day_time("nightly-2032-05-06-aaaaaaa").as_deref(),
            Some("07:22")
        );
        assert_eq!(
            same_day_time("nightly-2032-05-06-bbbbbbb").as_deref(),
            Some("18:05")
        );
    }

    #[test]
    fn an_installed_nightly_keeps_its_time_after_restart() {
        let current = nightly_release("nightly-2033-06-07-bbbbbbb", "2033-06-07", "18:05");
        let installed = [("nightly-2033-06-07-aaaaaaa".to_owned(), "07:22".to_owned())];
        let table = same_day_times(&[], &[current], &installed);
        assert_eq!(
            table
                .iter()
                .find(|(tag, _)| tag == "nightly-2033-06-07-aaaaaaa")
                .map(|(_, time)| time.as_str()),
            Some("07:22")
        );
        assert_eq!(
            table
                .iter()
                .find(|(tag, _)| tag == "nightly-2033-06-07-bbbbbbb")
                .map(|(_, time)| time.as_str()),
            Some("18:05")
        );
    }

    #[test]
    fn a_current_already_installed_nightly_stays_current_after_going_offline() {
        use crate::instances::Channel::Nightly;
        use std::process::Command;
        const ACTOR: &str = "SUNRISE_TEST_ACCEPT_ALREADY_INSTALLED_NIGHTLY";
        if std::env::var_os(ACTOR).is_none() {
            let root = tempfile::tempdir().unwrap();
            assert!(Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "nightly::tests::a_current_already_installed_nightly_stays_current_after_going_offline"])
                .env(ACTOR, "1")
                .env("SUNRISE_LAUNCHER_HOME", root.path().join("launcher"))
                .env("SUNRISE_HOME", root.path().join("game"))
                .status().unwrap().success());
            return;
        }
        let data = backend::data_dir();
        let versions = data.join("versions");
        let old = "nightly-2033-06-07-bbbbbbb".to_owned();
        let current = "nightly-2033-06-07-aaaaaaa".to_owned();
        let catalogue = [nightly_release(&current, "2033-06-07", "11:05")];
        for tag in [&old, &current] {
            let dir = backend::version_dir(tag);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(INSTALLED_TIME), "11:05").unwrap();
            fs::write(dir.join(".installed"), "").unwrap();
        }
        fs::write(
            data.join("settings.json"),
            serde_json::json!({"selected": old}).to_string(),
        )
        .unwrap();
        let profile = crate::instances::selection().unwrap();
        assert_eq!(profile.channel, Nightly);
        assert_eq!(
            profile.resolve_version(None).unwrap(),
            old,
            "pre-receipt saved choice"
        );
        assert!(!versions.join(CURRENT_INSTALL).exists());
        let (installed, known) = backend::play_installations(Some(&old));
        let online = crate::instances::play_target(
            profile.channel,
            &catalogue,
            &installed,
            known.as_ref(),
            Some(&old),
        )
        .unwrap();
        assert_eq!(online, &current);
        backend::note_play_catalogue(&catalogue).unwrap();
        assert_eq!(
            fs::read_to_string(versions.join(CURRENT_INSTALL)).unwrap(),
            current
        );
        assert_eq!(&profile.resolve_version(None).unwrap(), online);
        let next_day = "nightly-2033-06-08-ccccccc";
        fs::create_dir(backend::version_dir(next_day)).unwrap();
        fs::write(backend::version_dir(next_day).join(".installed"), "").unwrap();
        record_current(&versions, next_day).unwrap();
        backend::note_play_catalogue(&catalogue).unwrap();
        assert_eq!(
            fs::read_to_string(versions.join(CURRENT_INSTALL)).unwrap(),
            next_day
        );
    }

    #[test]
    fn an_installed_nightly_records_its_publication_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut release = nightly_release("nightly-2033-06-07-bbbbbbb", "2033-06-07", "18:05");
        release.published_at = "2033-06-07T18:05:42Z".into();
        record_install_time(dir.path(), &release).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(INSTALLED_TIME)).unwrap(),
            "18:05"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(INSTALLED_PUBLISHED)).unwrap(),
            "2033-06-07T18:05:42Z"
        );
    }

    #[test]
    fn a_rotated_nightly_keeps_its_full_publication_timestamp() {
        let mut early = nightly_release("nightly-2034-07-08-aaaaaaa", "2034-07-08", "12:30");
        early.published_at = "2034-07-08T12:30:10Z".into();
        let mut late = nightly_release("nightly-2034-07-08-bbbbbbb", "2034-07-08", "12:30");
        late.published_at = "2034-07-08T12:30:45Z".into();
        note_published(&[early]);
        note_published(&[late]);
        assert_eq!(
            published_at("nightly-2034-07-08-aaaaaaa").as_deref(),
            Some("2034-07-08T12:30:10Z")
        );
        assert_eq!(
            published_at("nightly-2034-07-08-bbbbbbb").as_deref(),
            Some("2034-07-08T12:30:45Z")
        );
    }

    #[test]
    fn a_full_commit_reads_as_nine_characters() {
        let full = "b1ff51d9e0123456789abcdef0123456789abcde";
        assert_eq!(short_commit(full), "b1ff51d9e…");
        assert_eq!(short_commit("v0.2.97"), "v0.2.97");
    }

    #[test]
    fn local_published_table_preserves_timestamps_across_nightly_rotation() {
        let mut early = nightly_release("nightly-2035-08-09-aaaaaaa", "2035-08-09", "12:30");
        early.published_at = "2035-08-09T12:30:10Z".into();
        let mut late = nightly_release("nightly-2035-08-09-bbbbbbb", "2035-08-09", "12:30");
        late.published_at = "2035-08-09T12:30:45Z".into();
        let mut table = PublishedTimestamps::default();
        table.note(&[early]);
        table.note(&[late]);
        assert_eq!(
            table.get("nightly-2035-08-09-aaaaaaa").as_deref(),
            Some("2035-08-09T12:30:10Z")
        );
        assert_eq!(
            table.get("nightly-2035-08-09-bbbbbbb").as_deref(),
            Some("2035-08-09T12:30:45Z")
        );
    }
}
