//! `sunrise-launcher <subcommand>`: the same steps as the window, from a terminal. Every
//! command goes through `backend`; this file only parses arguments and prints.
mod progress;
mod switch;
use progress::{native_progress, progress};
mod toolchains;
use toolchains::{java, rust, JavaCmd, RustCmd};
mod update;
use update::update;
mod accounts;
use accounts::{account_command, AccountCmd};
mod data;
use data::{data, DataCmd};
mod settings;
use settings::{on_off, set_setting, SETTING_KEYS};
mod servers;
use servers::{multiplayer, servers, MultiplayerCmd, ServersCmd};
mod mods;
use mods::{approve_mod_change, mods, ModsCmd};
mod instance;
use instance::{after_exit, instance, modpack, InstanceCmd, ModpackCmd};
mod content;
use content::{game_packs, screenshots, worlds, PacksCmd, ScreenshotsCmd, WorldsCmd};

use clap::{Parser, Subcommand};
use sunrise_launcher::account::{self, Accounts};
use sunrise_launcher::backend::{self, Release};
use sunrise_launcher::github_mods;
use sunrise_launcher::i18n::{tr, trf};
use sunrise_launcher::instances;
use sunrise_launcher::native_mods;
use sunrise_launcher::server_list;
use sunrise_launcher::storage::{Category, Storage};
#[derive(Clone, Debug)]
enum ClearStorage {
    All,
    Row(Category),
}
fn parse_storage_row(value: &str) -> Result<ClearStorage, String> {
    use clap::ValueEnum;
    if value == "all" {
        Ok(ClearStorage::All)
    } else {
        Category::ALL
            .into_iter()
            .find(|category| category.to_possible_value().unwrap().get_name() == value)
            .map(ClearStorage::Row)
            .ok_or_else(|| tr("Unknown").into())
    }
}
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
#[derive(Parser)]
#[command(
    name = crate::identity::EXECUTABLE,
    version = backend::VERSION,
    about = "Installs, prepares and starts Sunrise. Without a subcommand, opens the window."
)]
struct Cli {
    /// Print JSON instead of text, where a command has something to report
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

/// The `--profile` value. [`native_mods::Profile`] lives in the launcher's pure type crate, which
/// stays free of clap, so the command line parses its own one-variant name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum ProfileArg {
    Optimized,
}

impl From<ProfileArg> for native_mods::Profile {
    fn from(_: ProfileArg) -> Self {
        Self::Optimized
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a shell completion script to stdout
    Completions { shell: clap_complete::Shell },
    /// List Sunrise releases, or the installed versions
    Versions {
        /// Only the versions installed here (no network)
        #[arg(long)]
        installed: bool,
    },
    /// Download and install a Sunrise build
    Install {
        /// A build tag (e.g. 26.3-0.1.0, nightly, or dev-2026-11-21-0c02067), or "latest" for the profile's channel
        tag: String,
    },
    /// Install a local unsigned developer build from a trusted checkout
    #[command(about = tr("Install a local unsigned developer build from a trusted checkout"), after_help = tr("Example: sunrise-launcher developer-install CHECKOUT --program CHECKOUT/target/debug/sunrise-client --trust-unsigned\nThis executes the checkout's build code and an unsigned game with your rights. Trust both before continuing."), long_about = tr("Install a local unsigned developer build. This runs the checkout's build code through cargo run -p xtask -- package. Commit tracked changes and rebuild first. Build sunrise-ctl, sunrise-prepare and sunrise-server beside the game, and compile exporters into the checkout's exporters folder. Native builds use the saved local source commit, including unpublished commits. Select this base explicitly; automatic updates never replace it."))]
    DeveloperInstall(developer_builds::DeveloperInstall),
    /// Remove an installed version
    Remove { tag: String },
    /// Install a game program you have, such as a sunrise-client built on your fork's Actions,
    /// as its own version on an installed base's game files, and select it. A local unsigned
    /// developer base is allowed only when explicitly selected or named with --version.
    /// Nothing signed it: it runs with your rights, and updates never replace it
    #[command(
        after_help = "The program must match this computer's executable format and carry the exact `sunrise-game-executable;`\nidentity signature embedded by ordinary and launcher-generated games. It carries no mod API level or build id,\nand is not a signature of authenticity. Unmarked programs are refused."
    )]
    ImportBuild {
        /// The program (sunrise-client, sunrise-client.exe on Windows), or a zip holding it
        path: PathBuf,
        /// The installed base whose game files it plays with (default: the selected usable
        /// base, else the newest release or Nightly; developer bases need explicit selection)
        #[arg(long)]
        version: Option<String>,
        /// You picked this build yourself and trust it with full access to this computer
        #[arg(long)]
        trust_unsigned: bool,
    },
    /// Prepare a version's resources from your own Minecraft install, or from Mojang
    Prepare {
        /// The game folder of the official launcher, Prism Launcher, MultiMC, the Modrinth App,
        /// ATLauncher or GDLauncher (default: the launcher's, then every launcher's default)
        #[arg(long, conflicts_with = "download")]
        minecraft: Option<PathBuf>,
        /// Download Minecraft from Mojang for the signed-in account that owns it, instead of
        /// using an install
        #[arg(long)]
        download: bool,
        /// The installed version (default: the selected one)
        #[arg(long)]
        version: Option<String>,
    },
    /// Start the client (needs a signed-in Microsoft account that owns Minecraft: Java Edition)
    Play {
        /// Allow missing original Java asset/data jars to be downloaded from Modrinth
        #[arg(long)]
        allow_jar_downloads: bool,
        /// The profile to play (its name or id; default: the selected one)
        #[arg(long)]
        instance: Option<String>,
        /// Open or create this world (a folder under saves) at once
        #[arg(long)]
        world: Option<String>,
        /// Creative mode for a new world
        #[arg(long)]
        creative: bool,
        /// Join a friend's world with its invite code
        #[arg(long)]
        join: Option<String>,
        /// Join this server at once: an address, or a server of the list by name or position
        #[arg(long, conflicts_with_all = ["world", "join"])]
        server: Option<String>,
        /// An extra argument for the game, passed through as-is (repeatable), for example
        /// --server-thread
        #[arg(long = "arg", value_name = "ARG", allow_hyphen_values = true)]
        arg: Vec<String>,
        /// Play this launch as this signed-in account (its name or UUID), without changing
        /// the one `account select` picked: several games can play as several accounts at once
        #[arg(long, value_name = "WHO")]
        account: Option<String>,
    },
    /// Switch a profile to an exact installed and prepared game build
    Switch {
        /// The target build tag
        tag: String,
        /// The profile to switch (name or id; defaults to the selected profile)
        #[arg(long)]
        profile: Option<String>,
    },
    /// Run a dedicated server here; type commands into its console
    Server {
        /// Allow missing original Java asset/data jars to be downloaded from Modrinth
        #[arg(long)]
        allow_jar_downloads: bool,
        /// The server's folder (default: the one the launcher uses)
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
        /// Agree to the Minecraft EULA (https://aka.ms/MinecraftEULA)
        #[arg(long)]
        accept_eula: bool,
    },
    /// Show disposable storage by category as JSON. Builds must be idle
    ///
    /// Rust/Cargo/temp build-tool caches remain clearable. Game-accessible caches, installed-version
    /// removal and automatic runtime eviction retain files while any launch path lacks enforceable
    /// lifetime exclusion; an empty process scan is not authorization. Read-only reports stay available.
    /// Private launch records and raw captures are collected together only after all participating owners
    /// exit, under the exclusive permanent namespace gate. Published unit keys persist until exclusive
    /// unit removal. Registry entry counts are logged; lock entries are not age-collected.
    Storage {
        /// Request cleanup of build-files, prebuilt-games, spare-game-builds, rust-compiler, other-caches or all rows; game-accessible caches refuse cleanup without enforceable lifetime exclusion
        #[arg(long, value_parser = parse_storage_row)]
        clear: Option<ClearStorage>,
    },
    /// Generate the registry-only Cargo.lock for one native mod, without executing mod code
    NativeLock {
        folder: PathBuf,
        /// Explicitly replace an existing lock with newly resolved registry pins
        #[arg(long)]
        update_lock: bool,
    },
    /// Native mods (docs/modding/src/how-to/native-mods.md): compile flat Rust mod projects against the profile's exact
    /// Sunrise source revision with the launcher's own Rust, as the version the profile then
    /// plays. Not sandboxed: the mods' code runs with your rights. If there are no native mods to build, it exits successfully; a
    /// server export still fails, as no engine is made
    NativeBuild {
        /// Discover internal uses through compiler diagnostics. Prints resolved declarations
        /// as JSON for cargo sunrise-mod internals sync; never installs or selects a game
        #[arg(long, conflicts_with_all = ["server_output", "instance", "version", "profile"], requires = "folder")]
        sync_internals: bool,
        #[arg(long, requires = "folder", conflicts_with_all = ["sync_internals", "server_output", "instance", "version", "profile"])]
        catalog: bool,
        /// Copy discovery catalogs into this caller-owned empty directory
        #[arg(long, requires = "catalog")]
        catalog_output: Option<PathBuf>,
        /// A mod folder (sunrise-mod.toml with [native], Cargo.toml and src/), or a folder of them
        /// (default: the native mods installed and turned on in the profile)
        folder: Option<PathBuf>,
        /// Export mode: build a standalone dedicated-server engine into this folder instead of
        /// a profile's build. Needs a mod FOLDER; no profile is read or changed, and no
        /// launcher config is written
        #[arg(
            long = "server-output",
            value_name = "DIR",
            conflicts_with = "instance",
            requires = "folder"
        )]
        server_output: Option<PathBuf>,
        /// Allow exact-hash jar downloads into the local cache for the server export, which
        /// copies the jars its mods list into its own jars folder (the operator's copy; nothing we
        /// publish contains one); jars of mods from the Sunrise registry download without this
        #[arg(long, requires = "server_output")]
        allow_jar_downloads: bool,
        /// The profile whose installed native mods to build, and which then plays the build
        /// (default: the selected one)
        #[arg(long)]
        instance: Option<String>,
        /// For a profile, assert its current base. In server-export mode, select the installed
        /// game build to use (defaults to the newest installed release or Nightly)
        #[arg(long)]
        version: Option<String>,
        /// Build native code the registry did not review: a folder, or a profile's mods that were
        /// copied in by hand (asked again on every run)
        #[arg(long)]
        trust_native_code: bool,
        /// Native mods use the pinned SDK's optimized compilation settings.
        /// The game is never rebuilt by a mod build.
        #[arg(long, value_enum)]
        profile: Option<ProfileArg>,
        /// Build the horizon-mixins tool from its pinned source instead of downloading the
        /// prebuilt one: for CI, which keeps that path proven
        #[arg(long, hide = true)]
        mixins_from_source: bool,
    },
    /// Show a release's patch notes (default: the newest)
    Notes { tag: Option<String> },
    /// Show the website, roadmap and documentation links
    Links,
    /// Microsoft accounts: list them (the default), sign in, sign out, or pick the one that plays
    Account {
        #[command(subcommand)]
        action: Option<AccountCmd>,
    },
    /// Show the launcher's settings, or set one (the Settings page): selected-version TAG,
    /// game-architecture auto|x64|arm64,
    /// minecraft-dir PATH, server-dir PATH ("default" resets a folder),
    /// prune-old-versions on|off (delete unused old automatic downloads after an install, keeping
    /// the newest few), keep-versions N (how many of the
    /// newest to keep; "default" is 2), opening-animation on|off, cat on|off (the corner cat), proxy http://host:port|socks5://host:port
    /// ("default" removes it), while-playing stay|minimize|hide (what the window does while the game runs), language
    /// system|CODE (the window's language: en, fr, de, ja, pt-br... any language the launcher has;
    /// system follows the computer's; a wrong code lists them all), theme default|ID|local:SLUG
    /// (a built-in theme, or one of yours from the themes folder; "default" is Dawn; a wrong id
    /// lists them all), accent
    /// violet|blue|teal|green|amber|red|pink|#rrggbb|DEGREES (a named accent, any colour, or any
    /// hue; "theme" goes back to the theme's own), background PATH (your picture behind Home;
    /// "default" goes back to the theme's), scenery on|off (whether anything is behind the
    /// pages), panel-opacity PERCENT|default (glass panels over Home's background),
    /// reduced-graphics on|off (no glass blur or decorative motion), cache-limit-gib N (managed cache limit
    /// in GiB: 10, 25 (default), 50, 100; 0 disables automatic cleanup)
    #[command(after_help = tr("worlds rows|tiles: how Home lists the worlds under Continue playing."))]
    Settings {
        #[arg(value_parser = SETTING_KEYS)]
        key: Option<String>,
        /// The new value (with KEY)
        #[arg(requires = "key")]
        value: Option<String>,
    },
    /// Show or set the extra arguments the game starts with (for example --server-thread);
    /// quotes group a word with spaces in it
    GameArgs {
        /// Omit to show the current arguments
        #[arg(allow_hyphen_values = true)]
        args: Option<String>,
    },
    /// Upload a profile's latest.log to mclo.gs (a paste service for Minecraft logs) and print
    /// its link, for a bug report: only when the player asks. The home folder, the user name and
    /// the account names are taken out first; the link expires after 90 days unread
    ShareLog {
        /// The profile (default: the selected one)
        #[arg(long)]
        instance: Option<String>,
    },
    /// Delete a log shared with share-log: the "<id>:<token>" it printed
    UnshareLog { shared: String },
    /// Print the end of the game's log (logs/latest.log in the game folder), or its newest crash
    /// report
    Logs {
        /// How many lines of latest.log
        #[arg(long, default_value_t = 100)]
        lines: usize,
        /// The newest crash report instead
        #[arg(long)]
        crash: bool,
    },
    /// Look for a newer Sunrise release and launcher, and install them
    Update {
        /// Only report what is available
        #[arg(long)]
        check: bool,
    },
    /// Linux: put Sunrise in the applications menu (an entry and the icon, under
    /// $XDG_DATA_HOME), or take it out with --remove
    DesktopEntry {
        /// Remove the entry and the icon, and stop adding them back
        #[arg(long)]
        remove: bool,
    },
    /// Write a bug report to a file you attach to the issue by hand; nothing is sent
    /// (docs/telemetry.md)
    BugReport {
        text: String,
        /// Add the end of the game's log (names and worlds hidden, without chat)
        #[arg(long)]
        log: bool,
    },
    /// Opt-in data collection (docs/telemetry.md): what is allowed, what would be sent, and
    /// deleting it; all off until you allow a kind
    Data {
        #[command(subcommand)]
        action: Option<DataCmd>,
    },
    /// Profiles: separate game folders, each with its own worlds, options and packs
    #[command(visible_alias = "profile")]
    Instance {
        #[command(subcommand)]
        action: Option<InstanceCmd>,
    },
    /// A profile's single-player worlds: list them, back one up, copy or delete it
    Worlds {
        #[command(subcommand)]
        action: Option<WorldsCmd>,
    },
    /// A profile's resource and shader packs: list, add, turn on or off, remove
    Packs {
        #[command(subcommand)]
        action: Option<PacksCmd>,
    },
    /// Java for Minecraft: Java Edition profiles: list the runtimes, download Mojang's
    Java {
        #[command(subcommand)]
        action: Option<JavaCmd>,
    },
    /// The screenshots F2 took in a profile: list, delete
    Screenshots {
        #[command(subcommand)]
        action: Option<ScreenshotsCmd>,
    },
    /// The servers in a profile's Multiplayer list (servers.dat): list, add, remove
    Multiplayer {
        #[command(subcommand)]
        action: Option<MultiplayerCmd>,
    },
    /// Sunrise modpacks from the registry: list installed packs
    Modpack {
        #[command(subcommand)]
        action: Option<ModpackCmd>,
    },
    /// The mod browser: Java mods, resource packs and modpacks from Modrinth
    /// (CurseForge support is coming soon), each file checked against its checksum
    /// (docs/mod-browser.md)
    Mods {
        #[command(subcommand)]
        action: Option<ModsCmd>,
    },
    /// The portable Rust toolchain native mods compile with (docs/modding/src/how-to/native-mods.md): the launcher's
    /// own, in its data folder, never installed on the system
    Rust {
        #[command(subcommand)]
        action: Option<RustCmd>,
    },
    /// Servers hosted with sunrise-server (the Servers page): list them, create one, or run any
    /// sunrise-server command on one
    Servers {
        #[command(subcommand)]
        action: Option<ServersCmd>,
    },
    /// Serve the Model Context Protocol on stdin and stdout, for AI agents (refused while No AI
    /// is on)
    Mcp,
    /// Show or set No AI: on hides and turns off every AI feature (the companion, agent control,
    /// sunrise-ctl and both MCP servers); off, the default, makes them available
    NoAi {
        /// Omit to show the current state
        #[arg(value_parser = ["on", "off"])]
        state: Option<String>,
    },
    /// Measure the launcher: starts (the first is the cold one), page switching, idle CPU and
    /// memory, the release list fetch; writes a benchmark report to benchmarks/ in its data folder
    Benchmark {
        /// How many times to start it
        #[arg(long, default_value_t = 5)]
        repeats: usize,
    },
}

/// The CLI's definition, for the MCP tools generated from it (cli_tools.rs).
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// The commands that exist for AI agents: hidden and refused while No AI is on. Every other
/// command stays: the launcher, packagers and scripts need them, and none drives the game.
pub const AI_COMMANDS: [&str; 1] = ["mcp"];

/// Runs a subcommand and returns the exit code, or None when the window should open.
pub fn run() -> Option<i32> {
    use clap::FromArgMatches;
    let no_ai = backend::no_ai();
    let mut definition = command();
    if no_ai {
        // Hidden from the help too; a call still gets the refusal below.
        for name in AI_COMMANDS {
            definition = definition.mut_subcommand(name, |c| c.hide(true));
        }
    }
    let cli = Cli::from_arg_matches(&definition.get_matches()).unwrap_or_else(|e| e.exit());
    let command = cli.command?;
    let result = match command {
        Cmd::Mcp if no_ai => Err(sunrise_launcher::no_ai::REFUSAL.into()),
        Cmd::Mcp => crate::mcp::serve().map(|()| 0).map_err(Error::from),
        command => {
            let result = execute(command, cli.json);
            // A game it started has ended (attached) or none did (an error): a session file it
            // didn't read goes now.
            account::discard_unread_sessions();
            result
        }
    };
    Some(match result {
        Ok(code) => code,
        Err(e) => {
            // An error may name a file or folder as it was found: shown as text, never obeyed.
            eprintln!(
                "error: {}",
                sunrise_launcher::theme_files::for_terminal(&e.to_string())
            );
            1
        }
    })
}

/// Why a command failed: the message the CLI prints after `error: `, exactly as it was written.
#[derive(Debug)]
struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Anything the commands' own code fails with turns into its message, as it did into a `String`.
impl<E: Into<String>> From<E> for Error {
    fn from(message: E) -> Self {
        Self(message.into())
    }
}

fn json(value: serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}

/// Runs the fixture process in this terminal and reports how it ended.
#[cfg(test)]
fn attached(cmd: Command) -> Result<i32, String> {
    let (cmd, _runtime) = backend::prepare_command(cmd)?;
    attached_prepared(cmd)
}

fn attached_prepared(mut cmd: Command) -> Result<i32, String> {
    let status = backend::spawn_attached(&mut cmd)
        .and_then(|mut child| child.wait())
        .map_err(|e| format!("Could not start {:?}: {e}", cmd.get_program()))?;
    attached_status(status)
}

fn attached_status(status: std::process::ExitStatus) -> Result<i32, String> {
    #[cfg(unix)]
    {
        sunrise_launcher::process::supervisor::exit_code(status).map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        status.code().ok_or_else(|| format!("Exited with {status}"))
    }
}

fn release_json(r: &Release) -> serde_json::Value {
    serde_json::json!({
        "tag": r.tag, "name": r.name, "date": r.date, "prerelease": r.prerelease,
        "available_here": r.asset.is_some(), "installed": backend::installed().contains(&r.tag),
    })
}

fn execute(command: Cmd, as_json: bool) -> Result<i32, Error> {
    if matches!(command, Cmd::Versions { .. } | Cmd::Update { .. }) {
        for (tag, from) in backend::reuse_prepared_everywhere() {
            if !as_json {
                eprintln!("Prepared {tag} with the resources of {from}.");
            }
        }
    }
    match command {
        Cmd::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut self::command(),
                "sunrise-launcher",
                &mut std::io::stdout(),
            );
        }
        Cmd::Versions { installed: true } => {
            let installed = backend::installed();
            let selected = backend::resolve_version(None).ok();
            if as_json {
                json(serde_json::json!({ "installed": installed, "selected": selected }));
            } else if installed.is_empty() {
                println!("No version is installed.");
            }
            for tag in installed.iter().filter(|_| !as_json) {
                let mark = if selected.as_ref() == Some(tag) {
                    "*"
                } else {
                    " "
                };
                let state = if !backend::check_prepared(tag) {
                    "not prepared"
                } else if backend::has_sounds(tag) {
                    "prepared"
                } else {
                    "prepared, no sounds (prepare again)"
                };
                let label =
                    sunrise_launcher::developer_install::label(tag).unwrap_or_else(|| tag.clone());
                println!("{mark} {label}  {state}");
            }
        }
        Cmd::Versions { installed: false } => {
            let releases = backend::releases()?;
            if as_json {
                json(releases.iter().map(release_json).collect());
                return Ok(0);
            }
            let installed = backend::installed();
            for r in &releases {
                let state = match (installed.contains(&r.tag), r.asset.is_some()) {
                    (true, _) => "installed",
                    (false, true) => "",
                    (false, false) => "no build for this platform",
                };
                println!("{:<15} {}  {}", r.tag, r.date, state);
            }
        }
        Cmd::Install { tag } => {
            let label = format!("Installing {tag}");
            let tag = backend::install_tag(&tag, false, progress(label))?;
            if as_json {
                json(
                    serde_json::json!({ "installed": tag, "prepared": backend::check_prepared(&tag) }),
                );
            } else if backend::check_prepared(&tag) {
                println!(
                    "Installed {tag}. It is prepared already, from another installed version."
                );
            } else {
                println!(
                    "Installed {tag}. Prepare it next: sunrise-launcher prepare --version {tag}"
                );
            }
        }
        Cmd::Storage { clear } => {
            let storage = Storage::open().map_err(native_mods::cache_error)?;
            let (row, all) = match clear {
                Some(ClearStorage::Row(row)) => (Some(row), false),
                Some(ClearStorage::All) => (None, true),
                None => (None, false),
            };
            let report = storage.report(row, all).map_err(native_mods::cache_error)?;
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if row.is_some() || all {
                if let Some(kept) = report.rows.iter().flat_map(|row| &row.kept).next() {
                    return Err(kept.reason.clone().into());
                }
            }
        }
        Cmd::NativeLock {
            folder,
            update_lock,
        } => native_mods::registry_lock(&folder, update_lock)?,
        Cmd::NativeBuild {
            sync_internals,
            catalog,
            catalog_output,
            folder,
            server_output,
            allow_jar_downloads,
            instance,
            version,
            trust_native_code,
            profile,
            mixins_from_source,
        } => {
            let profile = profile.map(native_mods::Profile::from);
            native_mods::mixins_from_source(mixins_from_source);
            if catalog {
                native_permission(trust_native_code)?;
                let destination = catalog_output.ok_or("--catalog requires --catalog-output")?;
                let destination =
                    native_mods::catalog(&folder.expect("required by clap"), &destination)?;
                println!("{}", destination.display());
                return Ok(0);
            }
            if sync_internals {
                native_permission(trust_native_code)?;
                let entries = native_mods::sync_internals(&folder.expect("required by clap"))?;
                println!(
                    "{}",
                    serde_json::to_string(&entries).map_err(|error| error.to_string())?
                );
                return Ok(0);
            }
            let progress = native_progress(std::io::stderr().is_terminal());
            let steps = |step| {
                if let Some(label) = progress(step) {
                    eprintln!("{label}");
                }
            };
            if let Some(output) = server_output {
                native_permission(trust_native_code)?;
                // Export mode: a standalone server engine in `output`. No instance is read
                // or changed, and no launcher config is written; `folder` is required (clap).
                let folder = folder.unwrap();
                let base = match &version {
                    Some(v) => backend::resolve_version(Some(v))?,
                    // The instance's version is never looked at here: the newest installed
                    // release or Nightly, never a native build.
                    None => native_mods::base_for(None, &backend::installed(), false)
                        .ok_or_else(backend::no_version_installed)?,
                };
                let built = native_mods::build_server(
                    &base,
                    &folder,
                    &output,
                    profile,
                    allow_jar_downloads,
                    steps,
                )?;
                let built = std::path::absolute(&built).unwrap_or(built);
                let path = built.display().to_string();
                println!(
                    "{}",
                    trf("Built the server engine: {path}", &[("path", &path)])
                );
                println!(
                    "{}",
                    tr(
                        "Point the server at it: set engine.path to this file in the server's \
                        server.toml (docs/hosting.md); the launcher writes nothing itself."
                    )
                );
                return Ok(0);
            }
            let target = match &instance {
                Some(name) => instances::find(name)?,
                None => instances::selection()?,
            };
            let owner = if target.kind != instances::Kind::Pack
                && target.home.try_exists().map_err(|e| e.to_string())?
            {
                Some(sunrise_launcher::managed_play::Profile::open(
                    &target.home,
                    None,
                )?)
            } else {
                None
            };
            let active = owner
                .as_ref()
                .map(|owner| owner.active())
                .transpose()?
                .flatten();
            let base = match active {
                Some(active) => active.base().to_owned(),
                None => {
                    let plays = target.resolve_version(None)?;
                    let tag = native_mods::about(&plays).map_or(plays, |build| build.base);
                    native_mods::base_for(Some(&tag), &backend::installed(), false)
                        .ok_or_else(backend::no_version_installed)?
                }
            };
            if let Some(requested) = version.as_deref().filter(|tag| *tag != base) {
                return Err(trf("This profile uses {build}, not {requested}. Use Switch to change its game build.", &[("build", &base), ("requested", requested)]).into());
            }
            // A folder of source is code nobody reviewed. A profile's own mods build when the
            // registry verified them or the player confirmed them; the rest, such as files
            // copied in by hand, only when this run trusts them.
            if folder.is_some() {
                native_permission(trust_native_code)?;
            }
            if trust_native_code {
                trust_unreviewed_builds();
            }
            let target = match profile {
                Some(profile) => {
                    instances::edit(&target.id, |meta| meta.native_profile = Some(profile))?
                }
                None => target,
            };
            let explicit = folder.is_some() || profile.is_some();
            let folder = match folder {
                Some(folder) => folder,
                None => native_mods::installed_mods(&target.home)?,
            };
            let profile = explicit.then_some(target.native_profile);
            let built =
                match native_mods::build(&base, &folder, &target, profile, false, &native_mods::Cancellation::default(), steps, || backend::releases().ok()) {
                    // A content mod needs no build: the instance stays as it is.
                    Err(nothing @ native_mods::Failed::Nothing) => {
                        println!("{nothing}");
                        return Ok(0);
                    }
                    result => result,
                }
                .map_err(|error| {
                    if let native_mods::Failed::Worlds(stake) = &error {
                        return trf(
                            "{error}\n{stay}: nothing changed.",
                            &[
                                ("error", &stake.text),
                                ("stay", &native_mods::fit::stay(&stake.stay)),
                            ],
                        );
                    }
                    let error = String::from(error);
                    if error.starts_with(native_mods::MISMATCH) {
                        trf("{error} Run `cargo sunrise-mod update --sunrise {commit}` in each mod's folder to pin it to this game, then build again.", &[("error", &error), ("commit", &base)])
                    } else { error }
                })?;
            println!("{built}");
        }
        Cmd::DeveloperInstall(args) => developer_builds::install(args, as_json)?,
        Cmd::Remove { tag } => {
            backend::remove_version(&tag)?;
            println!("Removed {tag}.");
        }
        Cmd::ImportBuild {
            path,
            version,
            trust_unsigned,
        } => developer_builds::import(&path, version.as_deref(), trust_unsigned, as_json)?,
        Cmd::Prepare {
            minecraft,
            download,
            version,
        } => {
            let tag = backend::resolve_version(version.as_deref())?;
            let minecraft = backend::minecraft_dir(minecraft)?;
            let from = if download {
                backend::MinecraftFrom::Mojang
            } else {
                backend::MinecraftFrom::Install
            };
            let preparation = backend::prepare_lock(&tag)?;
            let (preparer, minecraft) = backend::preparer(
                &tag,
                &minecraft,
                from,
                &|line| eprintln!("{line}"),
                &preparation,
            )?;
            backend::sounds(&tag, &minecraft, &|line| println!("{line}"), &preparation);
            let (preparer, _runtime) = backend::prepare_command(preparer)?;
            let status = preparation
                .spawn_attached(preparer)
                .and_then(|mut child| child.wait())
                .map_err(|e| format!("Could not run the game's preparer: {e}"))?;
            let code = attached_status(status)?;
            if code != 0 {
                return Ok(code);
            }
            backend::finish_preparation(&tag, &preparation)?;
            println!("{tag} is prepared.");
        }
        Cmd::Play {
            allow_jar_downloads,
            instance,
            world,
            creative,
            join,
            server,
            arg,
            account,
        } => {
            let instance = instances::for_play(instance.as_deref())?;
            use sunrise_launcher::{jars, managed_play};
            let accounts = Accounts::load();
            let account = account
                .as_deref()
                .map(|who| {
                    accounts.find(who).ok_or_else(|| {
                        format!("No signed-in account {who}. See `sunrise-launcher account`.")
                    })
                })
                .transpose()?;
            let selected = account.or_else(|| accounts.selected());
            let add_arguments = |cmd: &mut Command| -> Result<(), String> {
                if let Some(code) = join {
                    backend::join_friend(cmd, &code)?;
                }
                if let Some(world) = world {
                    cmd.arg("--world").arg(world);
                }
                if let Some(server) = server {
                    let list = server_list::List::load(&instance.home);
                    let ip = server_list::find(&list, &server).map_or(server, |(_, s)| s.ip);
                    cmd.arg("--server").arg(ip);
                }
                if creative {
                    cmd.arg("--creative");
                }
                cmd.args(arg);
                Ok(())
            };
            let jar_permission = jars::Permission::headless(allow_jar_downloads);
            let vars = instance.vars();
            let (started, played, finished) = {
                let start = managed_play::Client {
                    instance: &instance,
                    accepted_account: selected.map(|a| a.uuid.as_str()),
                    explicit_account: account.is_some(),
                    piped: false,
                }
                .start(
                    || managed_play::headless_first(&instance),
                    |cmd| {
                        backend::append_game_args(cmd);
                        add_arguments(cmd)
                    },
                    |cmd| jars::prepare_for_command(cmd, jar_permission).map_err(|e| e.to_string()),
                    &native_mods::Cancellation::default(),
                    |progress| match progress {
                        managed_play::Progress::Prepare => {
                            backend::play_step("preparing the game's files")
                        }
                        managed_play::Progress::SignIn => backend::play_step("reading the sign-in"),
                        managed_play::Progress::Line(line) => eprintln!("{line}"),
                        _ => {}
                    },
                );
                let mut game = match start {
                    Ok(game) => game,
                    Err(managed_play::StartError::Hook(
                        error @ sunrise_launcher::launch::PreLaunchError::Exit(status),
                    )) => {
                        eprintln!("{error}");
                        return attached_status(status).map_err(Error::from);
                    }
                    Err(error) => return Err(error.into()),
                };
                for notice in game.registry_notices.iter().chain(&game.notice) {
                    eprintln!("{notice}");
                }
                let started = std::time::Instant::now();
                let status = game
                    .child
                    .wait()
                    .map_err(sunrise_launcher::process::child::unconfirmed_exit)?;
                game.runtime.game_exited();
                (started, attached_status(status), game.instance)
            };
            if let Err(error) = instances::finish_play(&finished, started.elapsed(), || {
                after_exit(&finished, &vars)
            }) {
                eprintln!(
                    "{}",
                    crate::trf("Play time not saved: {error}", &[("error", &error)])
                );
            }
            return played.map_err(Error::from);
        }
        Cmd::Switch { tag, profile } => {
            let instance = instances::read_profile(profile.as_deref())?;
            switch::run(&instance, &tag, as_json)?;
        }
        Cmd::Server {
            dir,
            version,
            accept_eula,
            allow_jar_downloads,
        } => {
            let tag = backend::resolve_version(version.as_deref())?;
            let dir = backend::server_dir(dir);
            // The server inherits this terminal, so typed lines reach its console.
            let mut cmd = backend::server(&tag, &dir, accept_eula)?;
            sunrise_launcher::jars::prepare_launch(
                &mut cmd,
                &Default::default(),
                sunrise_launcher::jars::Permission::headless(allow_jar_downloads),
            )
            .map_err(|error| error.to_string())?;
            let (cmd, _runtime) = backend::prepare_server_command(&tag, &dir, cmd)?;
            return attached_prepared(cmd).map_err(Error::from);
        }
        Cmd::Notes { tag } => {
            let r = backend::release_notes(tag.as_deref())?;
            if as_json {
                json(
                    serde_json::json!({ "tag": r.tag, "name": r.name, "date": r.date, "notes": r.body }),
                );
            } else {
                let title = if r.name.is_empty() { &r.tag } else { &r.name };
                println!(
                    "{title}\n{}  {}\n\n{}",
                    r.tag,
                    r.date,
                    backend::markdown_to_text(&r.body)
                );
            }
        }
        Cmd::Account { action } => account_command(action.unwrap_or(AccountCmd::List), as_json)?,
        Cmd::Data { action } => data(action.unwrap_or(DataCmd::Status), as_json)?,
        Cmd::Benchmark { repeats } => {
            let path = crate::perf::benchmark(repeats.max(1))?;
            if as_json {
                json(serde_json::json!({ "report": path }));
            } else {
                println!("{}", path.display());
            }
        }
        Cmd::Links => {
            let links = backend::links();
            if as_json {
                json(serde_json::to_value(&links).unwrap());
            } else {
                for (name, url) in [
                    ("Website", &links.website),
                    ("Roadmap", &links.roadmap),
                    ("Documentation", &links.docs),
                    ("Discord", &links.discord),
                ] {
                    println!("{name:<8} {}", if url.is_empty() { "(none)" } else { url });
                }
            }
        }
        Cmd::NoAi { state } => {
            if let Some(state) = state {
                backend::set_no_ai(state == "on")?;
            }
            let on = backend::no_ai();
            if as_json {
                json(serde_json::json!({ "no_ai": on }));
            } else if on {
                println!("No AI is on: every AI feature is hidden and off.");
            } else {
                println!("No AI is off: AI features are available.");
            }
        }
        Cmd::Settings { key, value } => {
            let mut settings = backend::Settings::load();
            if let Some(key) = key {
                let value = value.ok_or("settings KEY VALUE: the value is missing")?;
                set_setting(&mut settings, &key, &value)?;
                settings.save()?;
                sunrise_launcher::proxy::announce(settings.proxy.clone());
            }
            let all = settings_json(&settings);
            if as_json {
                json(all);
            } else {
                for (key, value) in all.as_object().into_iter().flatten() {
                    println!("{key}: {value}");
                }
            }
        }
        Cmd::GameArgs { args } => {
            let mut settings = backend::Settings::load();
            if let Some(line) = args {
                settings.game_args = line;
                settings.save()?;
            }
            let line = settings.game_args;
            if as_json {
                json(serde_json::json!({
                    "game_args": line,
                    "parsed": backend::split_args(&line),
                }));
            } else if line.is_empty() {
                println!("No extra game arguments.");
            } else {
                println!("Game arguments: {line}");
            }
        }
        Cmd::ShareLog { instance } => {
            use sunrise_launcher::game_log;
            let i = match instance {
                Some(name) => instances::find(&name)?,
                None => instances::selection()?,
            };
            let path = i.home.join("logs/latest.log");
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let names = game_log::names(&[&i.home, &backend::game_home()]);
            let text = game_log::private(&text, dirs::home_dir().as_deref(), &names);
            let shared = game_log::share(&game_log::api(), &text)?;
            if as_json {
                json(serde_json::json!({
                    "url": shared.url, "delete": format!("{}:{}", shared.id, shared.token),
                }));
            } else {
                println!("{}", shared.url);
                println!(
                    "Delete it: sunrise-launcher unshare-log {}:{}",
                    shared.id, shared.token
                );
            }
        }
        Cmd::UnshareLog { shared } => {
            let (id, token) = shared
                .split_once(':')
                .ok_or("Give the \"<id>:<token>\" share-log printed.")?;
            let api = sunrise_launcher::game_log::api();
            sunrise_launcher::game_log::unshare(
                &api,
                &sunrise_launcher::game_log::Shared {
                    url: String::new(),
                    id: id.into(),
                    token: token.into(),
                },
            )?;
            println!("Deleted the shared log {id}.");
        }
        Cmd::Logs { lines, crash } => {
            let (path, text) = backend::read_logs(lines, crash)?;
            if as_json {
                json(serde_json::json!({ "path": path, "text": text }));
            } else {
                print!("{text}");
            }
        }
        Cmd::Update { check } => update(check, as_json)?,
        Cmd::BugReport { text, log } => {
            let path = sunrise_launcher::bug_report::write(&text, log)?;
            if as_json {
                json(serde_json::json!({ "path": path }));
            } else {
                println!(
                    "Wrote {}: read it, then attach it to your issue.",
                    path.display()
                );
            }
        }
        Cmd::DesktopEntry { remove } => {
            let mut settings = backend::Settings::load();
            settings.desktop_entry = Some(!remove);
            settings.save()?;
            if remove {
                sunrise_launcher::desktop::remove()?;
                println!("Removed Sunrise from the applications menu.");
            } else {
                sunrise_launcher::desktop::install(backend::exe_path())?;
                println!("Sunrise is in the applications menu.");
            }
        }
        Cmd::Worlds { action } => worlds(
            action.unwrap_or(WorldsCmd::List { instance: None }),
            as_json,
        )?,
        Cmd::Packs { action } => game_packs(
            action.unwrap_or(PacksCmd::List {
                kind: None,
                instance: None,
            }),
            as_json,
        )?,
        Cmd::Screenshots { action } => screenshots(
            action.unwrap_or(ScreenshotsCmd::List { instance: None }),
            as_json,
        )?,
        Cmd::Multiplayer { action } => multiplayer(
            action.unwrap_or(MultiplayerCmd::List { instance: None }),
            as_json,
        )?,
        Cmd::Instance { action } => instance(action.unwrap_or(InstanceCmd::List), as_json)?,
        Cmd::Modpack { action } => return modpack(action.unwrap_or(ModpackCmd::List), as_json),
        Cmd::Mods { action } => mods(action.unwrap_or(ModsCmd::List { instance: None }), as_json)?,
        Cmd::Servers { action } => servers(action.unwrap_or(ServersCmd::List), as_json)?,
        Cmd::Rust { action } => rust(action.unwrap_or(RustCmd::Status), as_json)?,
        Cmd::Java { action } => java(action.unwrap_or(JavaCmd::List), as_json)?,
        Cmd::Mcp => unreachable!(),
    }
    Ok(0)
}

#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod catalog_tests;

mod native_permission_impl;
use native_permission_impl::{
    github_native, native_permission, trust_unreviewed_builds, GithubInstall,
};

mod developer_builds;

mod settings_output;
use settings_output::settings_json;
