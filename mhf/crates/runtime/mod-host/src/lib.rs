//! Runtime ownership and public C bindings for built-in and DLL Mods.

mod context;
pub mod data;
mod launch;
mod native;

pub use context::Context;
use context::Shared;
pub use launch::LaunchProvider;
pub use mhf_mod_api as api;
use mhf_mod_package::{Candidate, Kind, Resolved, Source};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::Arc,
};

pub type Result<T> = std::result::Result<T, String>;

/// Host-internal adapters. Rust objects never cross a DLL boundary.
/// Interface tables and their state remain valid until this object is dropped.
pub trait Module {
    fn prepare(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
    fn check(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
    fn attach(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
    fn stop(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
    fn detach(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
    /// Return extra references to the game DLL while preserving retired native
    /// buffers. The session releases its final reference before dropping us.
    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ModStatus {
    pub id: String,
    pub version: String,
    pub phase: &'static str,
    pub error: Option<String>,
}

struct Entry {
    module: Option<Box<dyn Module>>,
    context: Box<Context>,
    status: ModStatus,
    dependencies: BTreeSet<String>,
    participated: bool,
    stopped: bool,
    detach_called: bool,
    detached: bool,
    release_prepared: bool,
}

/// Owns providers, consumers, C contexts and native libraries for one session.
pub struct ModHost {
    entries: Vec<Entry>,
    shared: Arc<Shared>,
    next_phase: u32,
    startup_failed: bool,
    launch_finished: bool,
    shutting_down: bool,
    native_running: Cell<bool>,
    stopped: bool,
    detached: bool,
    release_prepared: bool,
    failed_cleanup: bool,
}

impl ModHost {
    pub fn load(
        resolved: Resolved,
        configuration: &BTreeMap<String, String>,
        mut builtin: impl FnMut(&Candidate) -> Result<Box<dyn Module>>,
    ) -> Result<Self> {
        let shared = Arc::new(Shared::new());
        let mut host = Self {
            entries: Vec::new(),
            shared,
            next_phase: api::PHASE_PREPARE,
            startup_failed: false,
            launch_finished: false,
            shutting_down: false,
            native_running: Cell::new(false),
            stopped: false,
            detached: false,
            release_prepared: false,
            failed_cleanup: false,
        };
        for candidate in resolved.mods {
            let id = candidate.manifest.id.clone();
            let context = Context::new(
                &candidate,
                configuration.get(&id).cloned().unwrap_or_default(),
                host.shared.clone(),
            );
            let module = with_owner(&id, || match &candidate.source {
                Source::Builtin => builtin(&candidate),
                Source::Directory(path) if candidate.manifest.kind == Kind::Native => {
                    let entry = candidate
                        .manifest
                        .entry
                        .as_ref()
                        .ok_or_else(|| format!("{id}: missing DLL entry"))?;
                    native::load(&path.join(entry), &context)
                }
                Source::Directory(path) => Ok(data::DataMod::new(path.clone()) as Box<dyn Module>),
            });
            let module = match module {
                Ok(module) => module,
                Err(error) => {
                    if let Err(cleanup) =
                        context.cleanup_hooks().and_then(|()| ensure_released(&id))
                    {
                        std::mem::forget(context);
                        host.retain();
                        return Err(format!("{id}: create: {error}; cleanup: {cleanup}"));
                    }
                    return Err(format!("{id}: create: {error}"));
                }
            };
            host.entries.push(Entry {
                module: Some(module),
                context,
                status: ModStatus {
                    id,
                    version: candidate.manifest.version.to_string(),
                    phase: "created",
                    error: None,
                },
                dependencies: candidate.manifest.dependencies.keys().cloned().collect(),
                participated: false,
                stopped: false,
                detach_called: false,
                detached: false,
                release_prepared: false,
            });
        }
        Ok(host)
    }

    pub fn statuses(&self) -> Vec<ModStatus> {
        self.entries
            .iter()
            .map(|entry| entry.status.clone())
            .collect()
    }
    pub fn set_game(&self, module: *mut c_void) {
        self.shared.set_game(module);
    }

    pub fn prepare(&mut self) -> Result<()> {
        self.run_phase(api::PHASE_PREPARE, "prepare", |module, context| {
            module.prepare(context)
        })
    }
    pub fn check(&mut self) -> Result<()> {
        self.run_phase(api::PHASE_CHECK, "check", |module, context| {
            module.check(context)
        })
    }
    pub fn attach(&mut self) -> Result<()> {
        self.run_phase(api::PHASE_ATTACH, "attach", |module, context| {
            module.attach(context)
        })
    }

    pub fn running(&self) {
        self.native_running.set(true);
        self.shared.set_phase(api::PHASE_RUNNING);
    }

    fn run_phase(
        &mut self,
        phase: u32,
        name: &'static str,
        call: impl Fn(&mut dyn Module, &Context) -> Result<()>,
    ) -> Result<()> {
        if self.startup_failed || self.shutting_down || self.next_phase != phase {
            return Err(format!("cannot {name} in the current Mod lifecycle state"));
        }
        self.shared.set_phase(phase);
        for entry in &mut self.entries {
            entry.participated = true;
            let publishes = matches!(phase, api::PHASE_PREPARE | api::PHASE_ATTACH);
            if publishes {
                entry.context.begin_registration();
            }
            let module = entry
                .module
                .as_mut()
                .expect("module exists before shutdown");
            let result = with_owner(&entry.status.id, || call(module.as_mut(), &entry.context));
            entry.context.end_registration();
            if let Err(error) = result {
                entry.context.discard_interfaces();
                entry.status.error = Some(error.clone());
                entry.status.phase = "failed";
                self.startup_failed = true;
                return Err(format!("{}: {name}: {error}", entry.status.id));
            }
            if publishes {
                entry.context.publish_interfaces();
            }
            entry.status.phase = name;
        }
        self.next_phase += 1;
        Ok(())
    }

    /// Stop consumers before providers. A failed consumer keeps its providers
    /// operational; unrelated modules can still stop. Successful calls are not
    /// repeated if the caller retries cleanup.
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.shutting_down = true;
        self.shared.set_phase(api::PHASE_STOP);
        let mut errors = Vec::new();
        let mut retained = BTreeSet::new();
        for index in (0..self.entries.len()).rev() {
            let entry = &mut self.entries[index];
            if !entry.participated || entry.stopped || retained.contains(&entry.status.id) {
                continue;
            }
            let module = entry
                .module
                .as_mut()
                .expect("module remains owned during cleanup");
            match with_owner(&entry.status.id, || module.stop(&entry.context)) {
                Ok(()) => {
                    entry.stopped = true;
                    entry.status.phase = "stopped";
                    entry.status.error = None;
                }
                Err(error) => {
                    entry.status.error = Some(error.clone());
                    entry.status.phase = "stop_failed";
                    errors.push(format!("{}: stop: {error}", entry.status.id));
                    retained.extend(self.dependencies_of(index));
                }
            }
        }
        self.stopped = errors.is_empty();
        self.failed_cleanup = !self.stopped;
        errors_result(errors)
    }

    /// Detach consumers before providers. `builtin_order` breaks ties between
    /// independent modules; it never overrides a dependency edge.
    ///
    /// # Safety
    /// The game and its native rendering/input callers must have stopped.
    pub unsafe fn detach(&mut self, builtin_order: &[&str]) -> Result<()> {
        if self.detached {
            return Ok(());
        }
        self.native_running.set(false);
        let stop_error = self.stop().err();
        self.shared.set_phase(api::PHASE_DETACH);
        let order = match self.teardown_order(builtin_order) {
            Ok(order) => order,
            Err(error) => {
                self.failed_cleanup = true;
                return Err(error);
            }
        };
        let mut errors: Vec<_> = stop_error.into_iter().collect();
        let mut retained = BTreeSet::new();
        for index in order {
            let entry = &mut self.entries[index];
            if !entry.participated
                || !entry.stopped
                || entry.detached
                || retained.contains(&entry.status.id)
            {
                continue;
            }
            let module = entry
                .module
                .as_mut()
                .expect("module remains owned during cleanup");
            let result = if entry.detach_called {
                Ok(())
            } else {
                with_owner(&entry.status.id, || module.detach(&entry.context))
                    .map(|()| entry.detach_called = true)
            }
            .and_then(|()| entry.context.cleanup_hooks())
            .and_then(|()| ensure_released(&entry.status.id));
            match result {
                Ok(()) => {
                    entry.detached = true;
                    entry.status.phase = "detached";
                    entry.status.error = None;
                }
                Err(error) => {
                    entry.status.error = Some(error.clone());
                    entry.status.phase = "detach_failed";
                    errors.push(format!("{}: detach: {error}", entry.status.id));
                    retained.extend(self.dependencies_of(index));
                }
            }
        }
        self.detached = errors.is_empty();
        self.failed_cleanup = !self.detached;
        errors_result(errors)
    }

    /// Return extra game DLL references, preserving every Mod instance and its
    /// retired buffers. After success the session releases its game module, then
    /// drops this host. A failure retains the complete session's Mod resources.
    ///
    /// # Safety
    /// All native callers must be stopped and detach must have succeeded. The
    /// session must still own its game DLL reference for the duration of this call.
    pub unsafe fn prepare_release(&mut self) -> Result<()> {
        if self.release_prepared {
            return Ok(());
        }
        if !self.detached {
            return Err("detach must complete before preparing game release".into());
        }
        let mut errors = Vec::new();
        let mut retained = BTreeSet::new();
        for index in (0..self.entries.len()).rev() {
            let entry = &mut self.entries[index];
            if !entry.participated || entry.release_prepared || retained.contains(&entry.status.id)
            {
                continue;
            }
            let module = entry
                .module
                .as_mut()
                .expect("module remains owned during cleanup");
            match with_owner(&entry.status.id, || module.prepare_release(&entry.context)) {
                Ok(()) => {
                    entry.release_prepared = true;
                    entry.status.phase = "release_prepared";
                    entry.status.error = None;
                }
                Err(error) => {
                    entry.status.error = Some(error.clone());
                    entry.status.phase = "release_failed";
                    errors.push(format!("{}: prepare release: {error}", entry.status.id));
                    retained.extend(self.dependencies_of(index));
                }
            }
        }
        self.release_prepared = errors.is_empty();
        self.failed_cleanup = !self.release_prepared;
        errors_result(errors)
    }

    fn dependencies_of(&self, index: usize) -> BTreeSet<String> {
        let mut dependencies = BTreeSet::new();
        let mut pending: Vec<_> = self.entries[index].dependencies.iter().cloned().collect();
        while let Some(id) = pending.pop() {
            if dependencies.insert(id.clone())
                && let Some(entry) = self.entries.iter().find(|entry| entry.status.id == id)
            {
                pending.extend(entry.dependencies.iter().cloned());
            }
        }
        dependencies
    }

    fn teardown_order(&self, preferred: &[&str]) -> Result<Vec<usize>> {
        let mut remaining: Vec<_> = (0..self.entries.len()).rev().collect();
        remaining.sort_by_key(|index| {
            preferred
                .iter()
                .position(|id| *id == self.entries[*index].status.id)
                .map_or(0, |rank| rank + 1)
        });
        let mut order = Vec::new();
        while !remaining.is_empty() {
            let next = remaining
                .iter()
                .position(|index| {
                    !remaining.iter().any(|consumer| {
                        self.entries[*consumer]
                            .dependencies
                            .contains(&self.entries[*index].status.id)
                    })
                })
                .ok_or("resolved Mod plan contains a dependency cycle")?;
            order.push(remaining.remove(next));
        }
        Ok(order)
    }

    /// Keep every module/context/library alive when native shutdown fails.
    pub fn retain(self) {
        std::mem::forget(self);
    }
}

impl Drop for ModHost {
    fn drop(&mut self) {
        if self.native_running.get() || self.failed_cleanup {
            std::mem::forget(std::mem::take(&mut self.entries));
            return;
        }
        if !self.detached
            && let Err(error) = unsafe { self.detach(&[]) }
        {
            eprintln!("Mod cleanup failed: {error}");
        }
        if !self.failed_cleanup
            && !self.release_prepared
            && let Err(error) = unsafe { self.prepare_release() }
        {
            self.failed_cleanup = true;
            eprintln!("Mod release failed: {error}");
        }
        if self.failed_cleanup {
            std::mem::forget(std::mem::take(&mut self.entries));
            return;
        }
        self.shared.set_phase(api::PHASE_DESTROY);
        // All contexts remain alive while consumers destroy themselves and may
        // still call provider interfaces. Each provider is destroyed afterwards.
        for entry in self.entries.iter_mut().rev() {
            with_owner(&entry.status.id, || drop(entry.module.take()));
        }
    }
}

fn with_owner<T>(id: &str, call: impl FnOnce() -> T) -> T {
    #[cfg(windows)]
    {
        mhf_hooks::with_owner(id, call)
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        call()
    }
}

fn ensure_released(id: &str) -> Result<()> {
    #[cfg(windows)]
    {
        mhf_hooks::ensure_released(id)
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        Ok(())
    }
}

fn errors_result(errors: Vec<String>) -> Result<()> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests;
