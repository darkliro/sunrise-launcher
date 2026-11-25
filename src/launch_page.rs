//! Launch… on Home's Instance row: what runs around the selected instance's game
//! (`sunrise_launcher::launch`): a command before it, a wrapper, and environment variables.
use super::servers_page::Field;
use super::*;

use sunrise_launcher::launch;

pub(super) struct LaunchPanel {
    /// The instance it is for: picking another closes it.
    instance: String,
    /// What Save did: Ok with the summary of what runs (None when nothing runs), or Err with reason.
    saved: Option<Result<Option<String>, String>>,
    /// The Linux switches (GameMode, MangoHud, dedicated card) as flipped, kept with Save like
    /// the commands: None until one is flipped.
    switches: Option<[bool; 3]>,
}

impl Launcher {
    /// What the hook fields hold now, when the panel is open for `id`: the commands before and
    /// after the game, the wrapper and the variables, as typed.
    pub(super) fn hooks_typed(&self, id: &str) -> Option<[String; 4]> {
        self.launch.as_ref().filter(|p| p.instance == id)?;
        let typed = |f: Field| self.servers.value(f).trim().to_string();
        Some([
            typed(Field::PreLaunch),
            typed(Field::Wrapper),
            typed(Field::PostExit),
            typed(Field::LaunchEnv),
        ])
    }

    /// The Linux switches as flipped in the open panel for `id`, when one was.
    pub(super) fn switches_typed(&self, id: &str) -> Option<[bool; 3]> {
        self.launch.as_ref().filter(|p| p.instance == id)?.switches
    }

    /// Parse the open command fields (and the flipped switches) without writing any instance
    /// settings.
    pub(super) fn parsed_launch(&self, id: &str) -> Option<Result<launch::Launch, String>> {
        let fields = self.hooks_typed(id)?;
        let saved = self.instances.list.iter().find(|i| i.id == id)?;
        let switches = self.switches_typed(id);
        Some(
            parse_commands(saved.launch.clone(), fields).map(|mut launch| {
                if let Some([gamemode, mangohud, gpu]) = switches {
                    launch.gamemode = gamemode;
                    launch.mangohud = mangohud;
                    launch.dedicated_gpu = gpu;
                }
                launch
            }),
        )
    }

    pub(super) fn save_launch(&mut self) {
        let Some(id) = self.launch.as_ref().map(|p| p.instance.clone()) else {
            return;
        };
        let Some(parsed) = self.parsed_launch(&id) else {
            return;
        };
        let saved = parsed.and_then(|launch| instances::edit(&id, |m| m.launch = launch));
        let saved = saved.map(|i| {
            self.reload_instances();
            launch::summary(&i.launch)
        });
        if let Some(p) = &mut self.launch {
            p.saved = Some(saved);
        }
    }
}

fn parse_commands(
    mut launch: launch::Launch,
    [pre, wrapper, post, env]: [String; 4],
) -> Result<launch::Launch, String> {
    launch.env = launch::parse_env(&env)?;
    launch.pre_launch = pre;
    launch.wrapper = wrapper;
    launch.post_exit = post;
    Ok(launch)
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn parsing_commands_is_an_unsaved_draft() {
        let saved = launch::Launch::default();
        let fields = [
            "backup".into(),
            "wrapper".into(),
            "after".into(),
            "MODE=play".into(),
        ];
        let draft = parse_commands(saved.clone(), fields).unwrap();
        assert!(saved.pre_launch.is_empty());
        assert!(saved.env.is_empty());
        assert_eq!(draft.pre_launch, "backup");
        assert_eq!(draft.wrapper, "wrapper");
        assert_eq!(draft.post_exit, "after");
        assert_eq!(draft.env.get("MODE").unwrap(), "play");
        assert!(parse_commands(
            saved,
            [
                String::new(),
                String::new(),
                String::new(),
                "not an assignment".into()
            ]
        )
        .is_err());
    }
}
