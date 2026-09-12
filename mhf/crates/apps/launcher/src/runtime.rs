//! Configuration and paths shared by the game host and application assembly.

#[cfg(feature = "base")]
use mhf_base::MhfConfig;
use mhf_config::{Registration, Store};
use mhf_game::{LaunchConfig, MhfLaunchParams32};
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct PreparedLaunch {
    pub game: LaunchConfig,
    #[cfg(feature = "base")]
    pub mhf: MhfConfig,
    pub store: Arc<Mutex<Store>>,
    pub mods_dir: PathBuf,
}

pub fn prepare(
    config_path: Option<PathBuf>,
    game_dir: Option<PathBuf>,
) -> Result<PreparedLaunch, String> {
    let invocation_dir = env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let launcher_dir = env::current_exe()
        .map_err(|error| format!("failed to determine executable path: {error}"))?
        .parent()
        .ok_or("executable has no parent directory")?
        .to_owned();
    let game_dir = game_dir.map_or_else(|| launcher_dir.clone(), |path| invocation_dir.join(path));
    let config_path = config_path.map_or_else(
        || invocation_dir.join("mhf.toml"),
        |path| invocation_dir.join(path),
    );
    let mut store = Store::load(config_path)?;
    store.register("mods", Registration::default())?;
    let mods: mhf_mod_package::RuntimeConfig = store
        .read("mods")?
        .try_into()
        .map_err(|error| format!("invalid Mod configuration: {error}"))?;
    let mods_dir = invocation_dir.join(&mods.directory);
    let store = Arc::new(Mutex::new(store));
    #[cfg(feature = "base")]
    let mhf = {
        let service = mhf_config::ConfigService::new(store.clone());
        // This temporary view borrows the service while producing owned settings.
        let config = unsafe { mhf_config::bind(service.api()) };
        mhf_base::register_config(config)?
    };
    #[cfg(feature = "base")]
    let params = launch_params(&mhf)?;
    // Without built-in Base, the external startup provider supplies game settings.
    #[cfg(not(feature = "base"))]
    let params = MhfLaunchParams32::default();
    if !game_dir.is_dir() {
        return Err(format!(
            "game directory does not exist: {}",
            game_dir.display()
        ));
    }
    Ok(PreparedLaunch {
        game: LaunchConfig {
            game_dir,
            params,
            mods,
        },
        #[cfg(feature = "base")]
        mhf,
        store,
        mods_dir,
    })
}

#[cfg(feature = "base")]
fn launch_params(config: &MhfConfig) -> Result<MhfLaunchParams32, String> {
    let mut params = MhfLaunchParams32 {
        preset_level: config.set.preset_level,
        custom: u32::from(config.set.custom),
        screen_mode: config.screen.mode.into(),
        window_width: config.screen.window_resolution.width,
        window_height: config.screen.window_resolution.height,
        fullscreen_width: config.screen.fullscreen_resolution.width,
        fullscreen_height: config.screen.fullscreen_resolution.height,
        display_character_limit: config.video.display_character_limit,
        use_dxt_textures: u32::from(config.video.use_dxt_textures),
        now_monitor_wh: u32::from(config.video.now_monitor_wh),
        graphics_version: config.video.graphics_version.into(),
        sound_disabled: u32::from(config.sound.disabled),
        sound_volume: config.sound.volume,
        inactive_sound_volume: config.sound.inactive_volume,
        minimized_sound_volume: config.sound.minimized_volume,
        sound_sample_rate: config.sound.sample_rate,
        sound_buffer_size: config.sound.buffer_size,
        language: config.localization.language.into(),
        font_quality: config.font.quality.into(),
        font_weight: u32::from(config.font.weight),
        draw_skip: u32::from(config.option.draw_skip),
        clog_disabled: u32::from(config.option.clog_disabled),
        use_proxy: u32::from(config.launch.use_proxy),
        use_ie_proxy: u32::from(config.launch.use_ie_proxy),
        proxy_configured: u32::from(config.launch.proxy_configured),
        proxy_port: u32::from(config.launch.proxy_port),
        server_selection: config.launch.server_selection,
        ..Default::default()
    };
    copy_c_string(
        "font name",
        &mut params.font_name[..],
        config.font.name.as_bytes(),
    )?;
    copy_c_string(
        "proxy address",
        &mut params.proxy_address,
        config.launch.proxy_address.to_string().as_bytes(),
    )?;
    Ok(params)
}

#[cfg(feature = "base")]
fn copy_c_string(field: &str, destination: &mut [u8], value: &[u8]) -> Result<(), String> {
    if value.len() >= destination.len() {
        return Err(format!(
            "{field} is {} bytes; at most {} bytes are supported",
            value.len(),
            destination.len().saturating_sub(1)
        ));
    }
    destination.fill(0);
    destination[..value.len()].copy_from_slice(value);
    Ok(())
}

#[cfg(all(test, feature = "base"))]
mod tests {
    use super::*;

    #[test]
    fn game_and_ini_bridge_share_the_fixed_hd_setting() {
        let directory = env::temp_dir().join(format!("mhf-app-config-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mhf.toml");
        std::fs::write(
            &path,
            "[video]\ngraphics_version = 'standard'\n[mods.'example.test']\nenabled = false\n",
        )
        .unwrap();
        let prepared = prepare(Some(path), Some(directory.clone())).unwrap();
        assert_eq!(prepared.game.params.graphics_version, 1);
        assert_eq!(prepared.mods_dir, env::current_dir().unwrap().join("mods"));
        assert_eq!(
            prepared.mhf.video.graphics_version,
            mhf_base::GraphicsVersion::HighDefinition
        );
        let store = prepared.store.lock().unwrap();
        assert_eq!(store.value("VIDEO", "GRAPHICS_VER").as_deref(), Some("1"));
        assert!(
            !store
                .section_names()
                .iter()
                .any(|name| name.eq_ignore_ascii_case("mods"))
        );
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
