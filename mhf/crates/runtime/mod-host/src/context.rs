use crate::{Result, api};
use mhf_mod_package::{Candidate, Source};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

pub(crate) struct Shared {
    interfaces: Mutex<BTreeMap<(String, String), usize>>,
    game: AtomicUsize,
    phase: AtomicU32,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            interfaces: Mutex::new(BTreeMap::new()),
            game: AtomicUsize::new(0),
            phase: AtomicU32::new(api::PHASE_PREPARE),
        }
    }
    pub(crate) fn set_game(&self, game: *mut c_void) {
        self.game.store(game as usize, Ordering::Release);
    }
    pub(crate) fn set_phase(&self, phase: u32) {
        self.phase.store(phase, Ordering::Release);
    }
    pub(crate) fn phase(&self) -> u32 {
        self.phase.load(Ordering::Acquire)
    }
    pub(crate) fn providers(&self, interface: &str) -> Vec<(String, usize)> {
        self.interfaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|((_, id), _)| id == interface)
            .map(|((provider, _), table)| (provider.clone(), *table))
            .collect()
    }
}

/// Stable per-Mod C context. Registry locks are never held over a Mod callback.
pub struct Context {
    api: api::HostV2,
    id: String,
    dependencies: BTreeSet<String>,
    configuration: String,
    root: String,
    shared: Arc<Shared>,
    registering: Cell<bool>,
    pending: RefCell<BTreeMap<String, usize>>,
    // A safe built-in registration keeps its callback alive even if the caller
    // releases its LaunchProvider handle before this context is destroyed.
    pub(crate) launch_providers: RefCell<Vec<Rc<crate::launch::State>>>,
    error: Mutex<String>,
    #[cfg(windows)]
    #[allow(clippy::vec_box)] // C group handles need stable addresses as the list grows.
    groups: RefCell<Vec<Box<Group>>>,
}

impl Context {
    pub(crate) fn new(
        candidate: &Candidate,
        configuration: String,
        shared: Arc<Shared>,
    ) -> Box<Self> {
        let root = match &candidate.source {
            Source::Directory(path) => path.clone(),
            Source::Builtin => std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(ToOwned::to_owned))
                .unwrap_or_default(),
        };
        let mut context = Box::new(Self {
            api: api::HostV2 {
                context: std::ptr::null_mut(),
                log,
                config,
                resource_root,
                last_error,
                register_interface,
                dependency,
                game_info,
                hooks: api::HookApiV1 {
                    prepare_group,
                    create_hook,
                    enable_group,
                    discard_group,
                },
            },
            id: candidate.manifest.id.clone(),
            dependencies: candidate.manifest.dependencies.keys().cloned().collect(),
            configuration,
            root: root.to_string_lossy().into_owned(),
            shared,
            registering: Cell::new(false),
            pending: RefCell::new(BTreeMap::new()),
            launch_providers: RefCell::new(Vec::new()),
            error: Mutex::new(String::new()),
            #[cfg(windows)]
            groups: RefCell::new(Vec::new()),
        });
        context.api.context = (&mut *context as *mut Self).cast();
        context
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn api(&self) -> &api::HostV2 {
        &self.api
    }
    pub fn config(&self) -> &str {
        &self.configuration
    }
    pub fn resource_root(&self) -> &str {
        &self.root
    }
    pub fn game(&self) -> api::GameInfoV2 {
        api::GameInfoV2 {
            module_base: self.shared.game.load(Ordering::Acquire) as *mut c_void,
            phase: self.shared.phase(),
        }
    }

    /// # Safety
    /// `table` and its referenced state must stay valid through this Mod's
    /// destruction, and must implement the public interface identified by `id`.
    pub unsafe fn register(&self, id: &str, table: *const c_void) -> Result<()> {
        if !self.registering.get() {
            return Err("interfaces can only be registered while preparing or attaching".into());
        }
        if table.is_null() {
            return Err(format!("interface {id} has no table"));
        }
        let key = (self.id.clone(), id.to_owned());
        if self
            .shared
            .interfaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&key)
            || self.pending.borrow().contains_key(id)
        {
            return Err(format!(
                "interface {id} is already registered by {}",
                self.id
            ));
        }
        self.pending
            .borrow_mut()
            .insert(id.to_owned(), table as usize);
        Ok(())
    }

    pub fn interface(&self, provider: &str, interface: &str) -> Result<*const c_void> {
        if provider != self.id && !self.dependencies.contains(provider) {
            return Err(format!(
                "{} does not declare dependency {provider}",
                self.id
            ));
        }
        self.shared
            .interfaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(provider.to_owned(), interface.to_owned()))
            .copied()
            .map(|pointer| pointer as *const c_void)
            .ok_or_else(|| format!("{provider} has not published interface {interface}"))
    }

    pub(crate) fn publish_interfaces(&self) {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        let mut registry = self
            .shared
            .interfaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (id, pointer) in pending {
            registry.insert((self.id.clone(), id), pointer);
        }
    }
    pub(crate) fn discard_interfaces(&self) {
        self.pending.borrow_mut().clear();
    }
    pub(crate) fn begin_registration(&self) {
        self.registering.set(true);
    }
    pub(crate) fn end_registration(&self) {
        self.registering.set(false);
    }
    fn fail(&self, error: impl Into<String>) -> api::Status {
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = error.into();
        api::ERROR
    }
    pub(crate) fn error(&self) -> String {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub(crate) fn clear_error(&self) {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn cleanup_hooks(&self) -> Result<()> {
        #[cfg(windows)]
        {
            let mut groups = std::mem::take(&mut *self.groups.borrow_mut());
            let mut errors = Vec::new();
            for group in groups.iter_mut().rev() {
                if let Err(error) = group.cleanup() {
                    errors.push(error);
                }
            }
            if !errors.is_empty() {
                *self.groups.borrow_mut() = groups;
                return Err(errors.join("; "));
            }
        }
        Ok(())
    }
}

unsafe fn context<'a>(pointer: *mut c_void) -> &'a Context {
    unsafe { &*pointer.cast::<Context>() }
}

unsafe extern "C" fn log(pointer: *mut c_void, level: u32, message: api::Str) {
    let context = unsafe { context(pointer) };
    let message = unsafe { message.as_str() };
    if level == api::LOG_ERROR {
        context.fail(message);
    }
    eprintln!("[{}:{level}] {message}", context.id);
}

pub(crate) unsafe fn copy_bytes(
    bytes: &[u8],
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    let Ok(length) = u32::try_from(bytes.len()) else {
        return api::ERROR;
    };
    unsafe {
        required.write(length);
    }
    if capacity < length {
        return api::BUFFER_TOO_SMALL;
    }
    if length != 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
        }
    }
    api::OK
}

unsafe extern "C" fn config(
    pointer: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe {
        copy_bytes(
            context(pointer).configuration.as_bytes(),
            buffer,
            capacity,
            required,
        )
    }
}
unsafe extern "C" fn resource_root(
    pointer: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe { copy_bytes(context(pointer).root.as_bytes(), buffer, capacity, required) }
}
unsafe extern "C" fn last_error(
    pointer: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe {
        copy_bytes(
            context(pointer).error().as_bytes(),
            buffer,
            capacity,
            required,
        )
    }
}
unsafe extern "C" fn register_interface(
    pointer: *mut c_void,
    id: api::Str,
    table: *const c_void,
) -> api::Status {
    let context = unsafe { context(pointer) };
    match unsafe { context.register(id.as_str(), table) } {
        Ok(()) => api::OK,
        Err(error) => context.fail(error),
    }
}
unsafe extern "C" fn dependency(
    pointer: *mut c_void,
    provider: api::Str,
    id: api::Str,
    out: *mut *const c_void,
) -> api::Status {
    let context = unsafe { context(pointer) };
    match context.interface(unsafe { provider.as_str() }, unsafe { id.as_str() }) {
        Ok(table) => {
            unsafe {
                out.write(table);
            }
            api::OK
        }
        Err(error) => {
            context.fail(error);
            api::NOT_FOUND
        }
    }
}
unsafe extern "C" fn game_info(pointer: *mut c_void, out: *mut api::GameInfoV2) -> api::Status {
    unsafe {
        out.write(context(pointer).game());
    }
    api::OK
}

#[cfg(windows)]
struct Group {
    name: String,
    native: mhf_hooks::NativeGroup,
    state: *mut c_void,
    drain: Option<api::DrainFn>,
    enabled: bool,
}

#[cfg(windows)]
impl Group {
    fn cleanup(&mut self) -> Result<()> {
        if self.native.is_empty() {
            return Ok(());
        }
        self.native.disable()?;
        if self.enabled
            && let Some(drain) = self.drain
        {
            let status = unsafe { drain(self.state) };
            if status != api::OK {
                return Err(format!("{}: drain failed with status {status}", self.name));
            }
        }
        unsafe { self.native.remove() }
    }
}

unsafe extern "C" fn prepare_group(
    pointer: *mut c_void,
    name: api::Str,
    state: *mut c_void,
    drain: Option<api::DrainFn>,
    out: *mut api::HookGroup,
) -> api::Status {
    let context = unsafe { context(pointer) };
    #[cfg(windows)]
    {
        if !context.registering.get() {
            return context.fail("Hook groups can only be prepared during prepare or attach");
        }
        let name = unsafe { name.as_str() }.to_owned();
        let mut group = Box::new(Group {
            native: mhf_hooks::NativeGroup::new(context.id.clone()),
            name,
            state,
            drain,
            enabled: false,
        });
        unsafe {
            out.write((&mut *group as *mut Group).cast());
        }
        context.groups.borrow_mut().push(group);
        api::OK
    }
    #[cfg(not(windows))]
    {
        let _ = (name, state, drain, out);
        context.fail("native hooks require Windows")
    }
}

unsafe extern "C" fn create_hook(
    pointer: *mut c_void,
    handle: api::HookGroup,
    target: *mut c_void,
    detour: *mut c_void,
    out: *mut *mut c_void,
) -> api::Status {
    let context = unsafe { context(pointer) };
    #[cfg(windows)]
    {
        if !context.registering.get() {
            return context.fail("hooks can only be created during prepare or attach");
        }
        let mut groups = context.groups.borrow_mut();
        let Some(group) = groups
            .iter_mut()
            .find(|group| (&***group as *const Group).cast_mut().cast::<c_void>() == handle)
        else {
            return context.fail("unknown Hook group");
        };
        if group.enabled {
            return context.fail("cannot add targets to an enabled Hook group");
        }
        match unsafe { group.native.create(&group.name, target, detour) } {
            Ok(trampoline) => {
                unsafe {
                    out.write(trampoline);
                }
                api::OK
            }
            Err(error) => context.fail(error),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (handle, target, detour, out);
        context.fail("native hooks require Windows")
    }
}

unsafe extern "C" fn enable_group(pointer: *mut c_void, handle: api::HookGroup) -> api::Status {
    let context = unsafe { context(pointer) };
    #[cfg(windows)]
    {
        if !context.registering.get() {
            return context.fail("hooks can only be enabled during prepare or attach");
        }
        let mut groups = context.groups.borrow_mut();
        let Some(group) = groups
            .iter_mut()
            .find(|group| (&***group as *const Group).cast_mut().cast::<c_void>() == handle)
        else {
            return context.fail("unknown Hook group");
        };
        if group.enabled {
            return context.fail("Hook group is already enabled");
        }
        // Even a partially enabled group needs disable/drain on rollback.
        group.enabled = true;
        match unsafe { group.native.enable() } {
            Ok(()) => api::OK,
            Err(error) => context.fail(error),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        context.fail("native hooks require Windows")
    }
}

unsafe extern "C" fn discard_group(pointer: *mut c_void, handle: api::HookGroup) -> api::Status {
    let context = unsafe { context(pointer) };
    #[cfg(windows)]
    {
        let mut groups = context.groups.borrow_mut();
        let Some(index) = groups
            .iter()
            .position(|group| (&**group as *const Group).cast_mut().cast::<c_void>() == handle)
        else {
            return context.fail("unknown Hook group");
        };
        if groups[index].enabled {
            return context.fail("enabled Hook groups are removed during Mod shutdown");
        }
        match unsafe { groups[index].native.remove() } {
            Ok(()) => {
                groups.remove(index);
                api::OK
            }
            Err(error) => context.fail(error),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        context.fail("native hooks require Windows")
    }
}
