use crate::{
    native,
    preview::Control,
    settings::{Settings, ViewSettings},
    ui::Workbench,
    worker::Worker,
};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, LaunchProvider, Module, Result};
use mhf_ui::{Overlay, OverlayRegistration, OverlayRegistry};
use std::{path::PathBuf, sync::Arc};
use windows::Win32::Foundation::HMODULE;

pub struct WorkbenchModule {
    startup: Option<LaunchProvider>,
    registry: OverlayRegistry,
    registration: Option<OverlayRegistration>,
    game_dir: PathBuf,
    data_root: PathBuf,
    export_root: PathBuf,
    control: Arc<Control>,
    view: ViewSettings,
    worker: Option<Arc<Worker>>,
    hook: Option<HookGuard<native::State>>,
}

impl WorkbenchModule {
    pub fn new(registry: OverlayRegistry, game_dir: PathBuf) -> Self {
        Self {
            startup: None,
            registry,
            registration: None,
            data_root: game_dir.join("dat"),
            export_root: game_dir.join("workbench-exports"),
            game_dir,
            control: Arc::new(Control::default()),
            view: ViewSettings::default(),
            worker: None,
            hook: None,
        }
    }
}

impl Module for WorkbenchModule {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        let settings: Settings = toml::from_str(context.config())
            .map_err(|error| format!("invalid workbench settings: {error}"))?;
        self.view = settings.view;
        self.control
            .set_preview_options(self.view.preview_options());
        self.data_root = self
            .game_dir
            .join(settings.data_root.unwrap_or_else(|| "dat".into()));
        self.export_root = self.game_dir.join(
            settings
                .export_root
                .unwrap_or_else(|| "workbench-exports".into()),
        );
        self.startup
            .insert(LaunchProvider::new(move |params, _global_data| {
                mhf_quest::configure_local_hunter(params, b"Workbench")?;
                Ok(true)
            }))
            .register(context)
    }

    fn attach(&mut self, context: &Context) -> Result<()> {
        let table = context.interface("mhf.config", "mhf.config.v1")?;
        // The dependency outlives this module; unregister drains and drops the
        // overlay before the provider can be released.
        let configuration = unsafe { mhf_config::bind(table.cast()) };
        self.hook = Some(unsafe {
            native::install(HMODULE(context.game().module_base), self.control.clone())
        }?);
        let worker = Arc::new(
            Worker::start(self.data_root.clone(), self.export_root.clone())
                .map_err(|error| format!("资源工作线程启动失败：{error}"))?,
        );
        self.worker = Some(worker.clone());
        let workbench = Workbench::new(
            self.control.clone(),
            worker,
            self.data_root.clone(),
            self.view,
            Some(configuration),
        );
        self.registration = Some(self.registry.register(Box::new(workbench)));
        Ok(())
    }

    fn stop(&mut self, _context: &Context) -> Result<()> {
        if let Some(registration) = &mut self.registration {
            registration.unregister();
        }
        // Unregister drains the active UI callback and drops its worker Arc.
        if let Some(worker) = self.worker.as_mut().and_then(Arc::get_mut) {
            worker.stop();
        }
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        if let Some(state) = self.hook.as_mut().and_then(HookGuard::retired_state_mut) {
            unsafe { state.prepare_release() }?;
        }
        Ok(())
    }
}

impl Overlay for Workbench {
    fn initialize(&mut self, context: &egui::Context) {
        mhf_font::install(context);
        egui_hunter::Theme::default().apply(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.show(ui);
    }
}
