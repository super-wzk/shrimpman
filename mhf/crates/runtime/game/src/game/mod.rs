//! One game lifecycle, independent of the selected startup provider.

mod native;

use crate::{LaunchConfig, MhfLaunchProfile};
use mhf_mod_host::{ModHost, ModStatus, Module};
use mhf_mod_package::{Candidate, Resolved};
use std::{collections::BTreeMap, env};

pub struct GameExit {
    /// None means the startup provider was cancelled before loading the game.
    pub code: Option<i32>,
    pub mods: Vec<ModStatus>,
}

struct GameSession {
    native: native::NativeGame,
    mods: Option<ModHost>,
}

/// Prepare Mods, invoke their startup provider and run the game with one
/// common lifetime. Built-in construction is supplied by the application.
pub fn run(
    prepared: &LaunchConfig,
    profile: &MhfLaunchProfile<'_>,
    resolved: Resolved,
    builtin: impl FnMut(&Candidate) -> Result<Box<dyn Module>, String>,
) -> Result<GameExit, String> {
    run_session(prepared, profile, resolved, builtin, |game| {
        Ok(unsafe { game.run() })
    })
}

fn run_session(
    prepared: &LaunchConfig,
    profile: &MhfLaunchProfile<'_>,
    resolved: Resolved,
    builtin: impl FnMut(&Candidate) -> Result<Box<dyn Module>, String>,
    entry: impl FnOnce(&mut native::NativeGame) -> Result<i32, String>,
) -> Result<GameExit, String> {
    let configuration: BTreeMap<String, String> = prepared
        .mods
        .modules
        .iter()
        .map(|(id, settings)| {
            toml::to_string(&settings.settings)
                .map(|text| (id.clone(), text))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    let invocation_dir = env::current_dir().map_err(|error| error.to_string())?;
    env::set_current_dir(&prepared.game_dir)
        .map_err(|error| format!("failed to enter {}: {error}", prepared.game_dir.display()))?;
    struct RestoreDirectory(std::path::PathBuf);
    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.0);
        }
    }
    let _directory = RestoreDirectory(invocation_dir);
    let mut game_dir = env::current_dir()
        .map_err(|error| error.to_string())?
        .to_string_lossy()
        .into_owned();
    if !game_dir.ends_with(['/', '\\']) {
        game_dir.push('\\');
    }
    let native = native::NativeGame::new(profile, &game_dir, prepared.params)?;
    let mods = ModHost::load(resolved, &configuration, builtin)?;
    let mut session = GameSession {
        native,
        mods: Some(mods),
    };
    let result = (|| {
        let mods = session.mods.as_mut().expect("session host is present");
        mods.prepare()?;
        if !session.native.launch(mods)? {
            return Ok(None);
        }
        let game = session.native.load(profile)?;
        mods.set_game(game.0);
        mods.check()?;
        mods.attach()?;
        mods.running();
        entry(&mut session.native).map(Some)
    })();
    let cleanup = session.finish();
    match (result, cleanup) {
        (Ok(code), Ok(mods)) => Ok(GameExit { code, mods }),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; cleanup: {cleanup}")),
    }
}

impl GameSession {
    fn finish(mut self) -> Result<Vec<ModStatus>, String> {
        let mut mods = self.mods.take().expect("session is only finished once");
        let cleanup = mods
            .stop()
            .and_then(|()| unsafe { mods.detach(&[]) })
            .and_then(|()| unsafe { mods.prepare_release() });
        if let Err(error) = cleanup {
            mods.retain();
            std::mem::forget(self);
            return Err(error);
        }
        // All additional game references have been returned. Retired states and
        // launch ABI storage remain alive while the game's DllMain runs.
        self.native.unload();
        mods.set_game(std::ptr::null_mut());
        let statuses = mods.statuses();
        drop(mods);
        Ok(statuses)
    }
}

#[cfg(test)]
mod tests;
