//! A build installed from a draft release's files is named as a test build and chosen only by its
//! exact tag: never as a profile's newest Dev build, and never as the base of an update.
use std::fs;
use sunrise_launcher::{
    backend, developer_install,
    instances::{self, Channel},
    native_mods,
};

#[test]
fn a_test_build_is_marked_and_never_the_newest() {
    let home = tempfile::tempdir().unwrap();
    for name in [
        "SUNRISE_HOME",
        "SUNRISE_LAUNCHER_HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "SUNRISE_LAUNCHER_TRASH",
        "SUNRISE_LAUNCHER_CLIPBOARD",
        "SUNRISE_LAUNCHER_DESKTOP",
    ] {
        std::env::set_var(name, home.path().join(name));
    }
    std::env::set_var("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1");
    let draft = "dev-2026-11-26-abcdef0";
    let public = "dev-2026-11-25-bdad86d";
    for tag in [draft, public] {
        fs::create_dir_all(backend::version_dir(tag)).unwrap();
    }
    fs::write(
        backend::version_dir(draft).join("draft-release.json"),
        r#"{"folder":"/drafts"}"#,
    )
    .unwrap();
    let installed = [draft.to_owned(), public.to_owned()];

    assert_eq!(
        instances::newest_installed(Channel::Dev, &installed),
        Some(&public.to_owned())
    );
    assert_eq!(
        native_mods::newest_installed(&installed),
        Some(&public.to_owned())
    );
    // An update offer is judged against the newest public install, not the test build.
    assert!(
        native_mods::newer_than("dev-2026-11-24-f8aa80e", &installed).as_deref() == Some(public)
    );

    let label = developer_install::label(draft).unwrap();
    assert_eq!(
        label,
        "Dev 2026-11-26 · abcdef0 · Test build from local files"
    );
    assert!(developer_install::instance_status(draft, true)
        .unwrap()
        .starts_with(&label));
    assert_eq!(developer_install::label(public), None);
}
