//! Selected-instance native instruction debugger. Captures are observations;
//! native helper inputs and world side effects are not an emulated game host.
mod hooks;
#[cfg(test)]
pub(super) mod tests;

use super::{State, get, monster, monster_ai};
use crate::provider::{AiDebugOperation, AiDebugSnapshot, AiTarget};
use mhf_ai_debug::{
    BreakpointKind, Debugger, InstanceId, Outcome, ProgramLocation, Recording, RunMode,
    ScriptImage, Snapshot, StopReason, Transition,
};
use mhf_monster::ai::{bind::ScriptBinding, bytecode, decompile::Memory, dsl::DebugInfo};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    mem::transmute,
    sync::{Arc, PoisonError},
};

pub(super) use hooks::{SIGNATURES, install};

pub(super) struct InstalledSource {
    pub target: AiTarget,
    pub descriptor: u32,
    pub debug_info: Arc<DebugInfo>,
    pub scripts: Vec<ScriptBinding>,
}

#[derive(Default)]
pub(super) struct Runtime {
    session: Option<Session>,
    frame: u64,
    next_revision: u64,
    error: Option<(AiTarget, String)>,
    archived: Option<Arc<AiDebugSnapshot>>,
}

struct Session {
    target: AiTarget,
    descriptor: u32,
    revision: u64,
    debug: Debugger,
    current: Snapshot,
    source: Arc<DebugInfo>,
    bindings: Vec<ScriptBinding>,
    images: BTreeMap<u32, ScriptImage>,
    unknown: BTreeMap<u32, u32>,
    pending: Option<Vec<u8>>,
    resume: Option<ParkedTurn>,
    entered: bool,
    detaching: bool,
    reason: Option<String>,
    wake: bool,
    pumped_frame: Option<u64>,
    cached: std::sync::OnceLock<Arc<AiDebugSnapshot>>,
}

struct ParkedTurn {
    continuation: hooks::Continuation,
    control: ControlStamp,
}

/// External mode changes may invalidate a parked turn while the world runs.
/// Scalar inputs may change, but resuming an old cursor in new lanes is invalid.
#[derive(PartialEq, Eq)]
struct ControlStamp {
    cursors: [u32; 15],
    lane: u16,
    stage: u8,
    main: u8,
    restart: u8,
    mode: u8,
}

impl ControlStamp {
    unsafe fn read(actor: usize) -> Self {
        Self {
            cursors: [
                2544, 2548, 2552, 2556, 2588, 2596, 2604, 2608, 2616, 2632, 2636, 2640, 2644, 2648,
                2652,
            ]
            .map(|offset| unsafe { get(actor + offset) }),
            lane: unsafe { get(actor + 3288) },
            stage: unsafe { get(actor + 2580) },
            main: unsafe { get(actor + 2576) },
            restart: unsafe { get(actor + 2622) },
            mode: unsafe { get(actor + 2680) },
        }
    }
}

impl Session {
    fn actor(&self) -> usize {
        self.target.pool as usize + usize::from(self.target.slot) * monster::STRIDE
    }

    unsafe fn valid(&self, state: &State) -> bool {
        let actor = self.actor();
        unsafe {
            state.read::<u32>(monster::POOL) == self.target.pool
                && self.target.pool != 0
                && get::<u8>(actor) != 0
                && get::<u32>(actor + 3448) == self.target.serial
                && get::<u32>(actor + 1656) == self.target.model
                && get::<u8>(actor + 3) == self.target.species
                && get::<u32>(actor + 2544) == self.descriptor
        }
    }

    fn location(&mut self, address: u32) -> Option<ProgramLocation> {
        if address == 0 {
            return None;
        }
        let (script, offset) = if let Some(binding) = self.bindings.iter().find(|binding| {
            address >= binding.address
                && u64::from(address - binding.address) < binding.length as u64
        }) {
            (binding.node as u32, address - binding.address)
        } else {
            let next_script = mhf_monster::ai::MAX_NODES as u32 + self.unknown.len() as u32;
            let script = *self.unknown.entry(address).or_insert(next_script);
            (script, 0)
        };
        Some(ProgramLocation {
            revision: self.revision,
            script,
            offset,
        })
    }

    unsafe fn capture(&mut self, address: u32, frame: u64) -> Snapshot {
        let actor = self.actor();
        let mut fields = BTreeMap::new();
        for (name, offset) in [
            ("main_state", 2576),
            ("stage", 2580),
            ("command_kind", 2581),
            ("target", 2612),
            ("request", 2682),
            ("request_priority", 3188),
            ("mode", 2680),
            ("detected", 2684),
            ("tracked", 2687),
            ("events", 2823),
            ("takeover", 2602),
            ("restart", 2622),
            ("action_group", 21),
            ("action_id", 20),
            ("action_stage", 5),
            ("rage", 2726),
            ("command_mode", 2739),
            ("interpreter_status", 3182),
        ] {
            fields.insert(name.into(), i64::from(unsafe { get::<u8>(actor + offset) }));
        }
        for (name, offset) in [
            ("lane", 3288),
            ("delay", 3228),
            ("area", 2040),
            ("target_arg1", 2582),
            ("target_arg2", 2584),
        ] {
            fields.insert(
                name.into(),
                i64::from(unsafe { get::<u16>(actor + offset) }),
            );
        }
        for (name, offset) in [
            ("area_timer", 2910),
            ("attack_timer", 2912),
            ("flash_timer", 2914),
        ] {
            fields.insert(
                name.into(),
                i64::from(unsafe { get::<i16>(actor + offset) }),
            );
        }
        for (name, offset) in [
            ("recovery", 2696),
            ("foraging", 2700),
            ("request_countdown", 2704),
        ] {
            fields.insert(
                name.into(),
                i64::from(unsafe { get::<i32>(actor + offset) }),
            );
        }
        for (name, offset) in [
            ("position_x_bits", 172),
            ("position_y_bits", 176),
            ("position_z_bits", 180),
        ] {
            fields.insert(
                name.into(),
                i64::from(unsafe { get::<u32>(actor + offset) }),
            );
        }
        Snapshot {
            instance: InstanceId {
                session: self.target.epoch,
                slot: u32::from(self.target.slot),
                generation: self.target.serial,
            },
            pc: self.location(address),
            frame,
            fields,
        }
    }

    fn instruction_bytes(&mut self, address: u32) -> Result<Vec<u8>, String> {
        // Logical widths are used only to label captured bytes, never to advance
        // the native cursor or scan a native conditional.
        let mut bytes = Vec::new();
        for index in 0..32 {
            bytes.extend(
                monster_ai::Live
                    .bytes(address.checked_add(index).ok_or("脚本地址溢出")?, 1)
                    .map_err(|e| e.to_string())?,
            );
            if let Ok(length) = bytecode::instruction_len(&bytes)
                && length <= bytes.len()
            {
                bytes.truncate(length);
                let pc = self.location(address).ok_or("空脚本游标")?;
                let image_count = self.images.len();
                match self.images.entry(pc.script) {
                    Entry::Occupied(image) => {
                        let start = pc.offset as usize;
                        if image.get().bytes.get(start..start + bytes.len())
                            != Some(bytes.as_slice())
                        {
                            return Err("脚本字节已变化，请重新附加调试器".into());
                        }
                    }
                    Entry::Vacant(image) => {
                        if image_count >= 4096 {
                            return Err("已达到 4096 个脚本记录上限，请重新附加".into());
                        }
                        image.insert(ScriptImage {
                            revision: self.revision,
                            script: pc.script,
                            name: format!("native_{}", pc.script),
                            bytes: bytes.clone().into(),
                            source: None,
                            source_spans: Arc::default(),
                        });
                    }
                }
                return Ok(bytes);
            }
        }
        Err("无法确定当前指令的编码边界".into())
    }

    fn complete(&mut self, after: Snapshot, outcome: Outcome) {
        self.cached.take();
        let before = std::mem::replace(&mut self.current, after);
        if let Some(bytes) = self.pending.take() {
            let note = match outcome {
                Outcome::Yield if bytes[0] == 5 => "动作请求后让出",
                Outcome::Yield => "本轮解释结束",
                Outcome::Reset => "停止并复位",
                _ => "",
            }
            .to_string();
            if let Err(error) = self.debug.after_instruction(Transition {
                before,
                after: self.current.clone(),
                opcode: bytes[0],
                operands: bytes[1..].to_vec(),
                outcome,
                note,
            }) {
                self.reason = Some(error.to_string());
            }
        }
    }

    fn snapshot(&self) -> Arc<AiDebugSnapshot> {
        self.cached
            .get_or_init(|| Arc::new(self.make_snapshot()))
            .clone()
    }

    fn archived_snapshot(&self, reason: &str) -> Arc<AiDebugSnapshot> {
        let mut snapshot = (*self.snapshot()).clone();
        snapshot.attached = false;
        snapshot.paused = true;
        snapshot.reason = reason.into();
        Arc::new(snapshot)
    }

    fn make_snapshot(&self) -> AiDebugSnapshot {
        let mut recording = self
            .debug
            .trace
            .recording()
            .unwrap_or_else(|_| Recording::empty(self.current.clone()));
        recording.scripts = self.images.values().cloned().collect();
        AiDebugSnapshot {
            target: self.target,
            attached: true,
            state: self.current.clone(),
            paused: self.debug.mode() == RunMode::Paused,
            reason: self
                .reason
                .clone()
                .unwrap_or_else(|| match self.debug.stop_reason() {
                    Some(StopReason::User) => "已暂停此 AI；世界继续运行".into(),
                    Some(StopReason::Breakpoint(id)) => format!("命中断点 {id}；世界继续运行"),
                    Some(StopReason::StepComplete) => "指令单步完成".into(),
                    Some(StopReason::Yielded) => "本轮解释已让出".into(),
                    Some(StopReason::BudgetExhausted) => "达到调试执行预算".into(),
                    Some(StopReason::Invalidated(reason)) => reason.clone(),
                    None => "正在记录所选实例的原生指令".into(),
                }),
            recording,
            breakpoints: self.debug.breakpoints().to_vec(),
            debug_info: self.source.clone(),
        }
    }
}

pub(super) fn snapshot(state: &State) -> Option<Arc<AiDebugSnapshot>> {
    let runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some((target, reason)) = &runtime.error {
        return Some(Arc::new(AiDebugSnapshot {
            target: *target,
            attached: false,
            state: Snapshot::default(),
            paused: true,
            reason: reason.clone(),
            recording: Recording::empty(Snapshot::default()),
            breakpoints: Vec::new(),
            debug_info: Arc::default(),
        }));
    }
    runtime
        .session
        .as_ref()
        .map(Session::snapshot)
        .or_else(|| runtime.archived.clone())
}

pub(super) fn invalidate(state: &State, reason: &str) {
    hooks::select_actor(0);
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(session) = runtime.session.take() {
        runtime.archived = Some(session.archived_snapshot(reason));
        runtime.error = None;
    }
}

pub(super) fn invalidate_target(state: &State, target: AiTarget, reason: &str) {
    let selected = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .session
        .as_ref()
        .is_some_and(|session| session.target == target);
    if selected {
        invalidate(state, reason);
    }
}

pub(super) unsafe fn command(
    state: &State,
    target: AiTarget,
    operation: AiDebugOperation,
    source: Option<&InstalledSource>,
) {
    let result = unsafe { apply_command(state, target, operation, source) };
    if let Err(error) = result {
        let mut runtime = state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(session) = &mut runtime.session {
            session.cached.take();
            session.reason = Some(error);
        } else {
            runtime.error = Some((target, error));
        }
    }
}

unsafe fn apply_command(
    state: &State,
    target: AiTarget,
    operation: AiDebugOperation,
    source: Option<&InstalledSource>,
) -> Result<(), String> {
    if matches!(operation, AiDebugOperation::Attach) {
        unsafe { detach(state) };
        let mut runtime = state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        runtime.next_revision += 1;
        let revision = runtime.next_revision;
        let actor = target.pool as usize + usize::from(target.slot) * monster::STRIDE;
        let descriptor = unsafe { get::<u32>(actor + 2544) };
        let mut session = Session {
            target,
            descriptor,
            revision,
            debug: Debugger::new(1024),
            current: Snapshot::default(),
            source: Arc::default(),
            bindings: Vec::new(),
            images: BTreeMap::new(),
            unknown: BTreeMap::new(),
            pending: None,
            resume: None,
            entered: false,
            detaching: false,
            reason: None,
            wake: false,
            pumped_frame: None,
            cached: std::sync::OnceLock::new(),
        };
        if !unsafe { session.valid(state) } {
            return Err("怪物实例已变化，请重新选择".into());
        }
        if let Some(source) =
            source.filter(|source| source.target == target && source.descriptor == descriptor)
        {
            session.source = source.debug_info.clone();
            session.bindings = source.scripts.clone();
            Arc::make_mut(&mut session.source)
                .mappings
                .retain(|mapping| {
                    session
                        .bindings
                        .iter()
                        .any(|binding| binding.node == mapping.script)
                });
            for binding in &session.bindings {
                let bytes = monster_ai::Live
                    .bytes(binding.address, binding.length)
                    .map_err(|e| e.to_string())?;
                let mapping = session.source.lookup(binding.node, 0);
                let name = mapping.map_or_else(
                    || format!("script_{}", binding.node),
                    |m| m.source.function.clone(),
                );
                let source = mapping
                    .and_then(|m| {
                        session
                            .source
                            .files
                            .iter()
                            .find(|f| f.path == m.source.path)
                    })
                    .map(|f| Arc::from(f.source.as_str()));
                let source_spans = session
                    .source
                    .mappings
                    .iter()
                    .filter(|m| m.script == binding.node)
                    .map(|m| mhf_ai_debug::SourceSpan {
                        offset_start: m.start as u32,
                        offset_end: m.end as u32,
                        path: m.source.path.clone(),
                        line: m.source.line as u32,
                        column: m.source.column as u32,
                        source_start: m.source.byte_start as u32,
                        source_end: m.source.byte_end as u32,
                    })
                    .collect();
                session.images.insert(
                    binding.node as u32,
                    ScriptImage {
                        revision,
                        script: binding.node as u32,
                        name,
                        bytes: bytes.into(),
                        source,
                        source_spans,
                    },
                );
            }
        }
        let cursor = unsafe { active_cursor(actor) };
        session.current = unsafe { session.capture(cursor, runtime.frame) };
        session.debug.pause();
        runtime.error = None;
        runtime.session = Some(session);
        hooks::select_actor(actor);
        return Ok(());
    }
    if matches!(operation, AiDebugOperation::Detach) {
        unsafe { detach(state) };
        return Ok(());
    }
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let session = runtime
        .session
        .as_mut()
        .filter(|s| s.target == target)
        .ok_or("请先附加到所选怪物")?;
    if !unsafe { session.valid(state) } {
        return Err("怪物或 AI 已变化，请重新附加".into());
    }
    if matches!(
        session.debug.stop_reason(),
        Some(StopReason::Invalidated(_))
    ) && matches!(
        operation,
        AiDebugOperation::Continue
            | AiDebugOperation::StepInstruction
            | AiDebugOperation::RunUntilYield
    ) {
        return Err("暂停期间原生控制状态已变化，请重新附加".into());
    }
    session.reason = None;
    session.cached.take();
    match operation {
        AiDebugOperation::Pause => session.debug.pause(),
        AiDebugOperation::Continue => session.debug.resume(),
        AiDebugOperation::StepInstruction => {
            session.debug.step();
            session.wake = true;
        }
        AiDebugOperation::RunUntilYield => {
            session.debug.run_until_yield(999);
            session.wake = true;
        }
        AiDebugOperation::ClearTrace => session.debug.trace.clear(),
        AiDebugOperation::SetBreakpoints(breakpoints) => session
            .debug
            .set_breakpoints(breakpoints)
            .map_err(|e| e.to_string())?,
        AiDebugOperation::SourceBreakpoint { path, line } => {
            let addresses: Vec<_> = session
                .source
                .positions(&path, line)
                .flat_map(|mapping| {
                    session
                        .bindings
                        .iter()
                        .filter(move |binding| binding.node == mapping.script)
                        .map(move |binding| binding.address + mapping.start as u32)
                })
                .collect();
            let positions: std::collections::BTreeSet<_> = addresses
                .into_iter()
                .filter_map(|address| session.location(address))
                .collect();
            if positions.is_empty() {
                return Err("此行没有可执行的已绑定指令；先应用当前工程".into());
            }
            let existing: Vec<_> = session
                .debug
                .breakpoints()
                .iter()
                .filter(
                    |b| matches!(&b.kind,BreakpointKind::Location(pc) if positions.contains(pc)),
                )
                .map(|b| b.id)
                .collect();
            if existing.is_empty() {
                for pc in positions {
                    session
                        .debug
                        .add_breakpoint(BreakpointKind::Location(pc), None);
                }
            } else {
                for id in existing {
                    session.debug.remove_breakpoint(id);
                }
            }
        }
        AiDebugOperation::Attach | AiDebugOperation::Detach => unreachable!(),
    }
    Ok(())
}

unsafe fn detach(state: &State) {
    let (archived, actor) = {
        let mut runtime = state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let archived = runtime
            .session
            .as_ref()
            .map(|session| session.archived_snapshot("调试器已分离；保留最后一段录制"));
        runtime.error = None;
        let actor = runtime.session.as_mut().and_then(|session| {
            if session.resume.as_ref().is_some_and(|resume| {
                resume.control == unsafe { ControlStamp::read(session.actor()) }
            }) && unsafe { session.valid(state) }
            {
                session.detaching = true;
                session.debug.resume();
                Some(session.actor())
            } else {
                if session.wake && unsafe { session.valid(state) } {
                    unsafe { super::put(session.actor() + 2601, 1_u8) };
                }
                None
            }
        });
        (archived, actor)
    };
    if let Some(actor) = actor {
        let original: unsafe extern "thiscall" fn(usize) -> i8 =
            unsafe { transmute(state.monster_ai) };
        unsafe { original(actor) };
    }
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    runtime.session = None;
    hooks::select_actor(0);
    if archived.is_some() {
        runtime.archived = archived;
    }
}

/// Called before the native frame, on the game thread, without holding the
/// debugger mutex. Only a parked turn or an explicit single-step is pumped.
pub(super) unsafe fn before_frame(state: &State, targets: &[AiTarget]) {
    let actor =
        {
            let mut runtime = state
                .ai_debug
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            runtime.frame = runtime.frame.wrapping_add(1);
            let frame = runtime.frame;
            let Some(session) = runtime.session.as_mut() else {
                return;
            };
            if !targets.contains(&session.target) || !unsafe { session.valid(state) } {
                runtime.archived =
                    Some(session.archived_snapshot("实例、任务或 AI 版本已变化，调试会话已结束"));
                runtime.session = None;
                hooks::select_actor(0);
                return;
            }
            if session.resume.as_ref().is_some_and(|resume| {
                resume.control != unsafe { ControlStamp::read(session.actor()) }
            }) {
                session.resume = None;
                session.cached.take();
                session.wake = false;
                session
                    .debug
                    .invalidate("暂停期间原生模式、lane 或续行已变化，请重新附加");
            }
            if session.debug.mode() != RunMode::Paused && session.resume.is_some() {
                session.pumped_frame = Some(frame);
                Some(session.actor())
            } else {
                if session.debug.mode() != RunMode::Paused && session.wake {
                    // em_ai_tick clears this latch even when select_action was
                    // suppressed by Pause. Re-arm it without bypassing delay ticks.
                    unsafe { super::put(session.actor() + 2601, 1_u8) };
                }
                None
            }
        };
    if let Some(actor) = actor {
        let original: unsafe extern "thiscall" fn(usize) -> i8 =
            unsafe { transmute(state.monster_ai) };
        unsafe { original(actor) };
    }
}

pub(super) fn should_suspend(state: &State, actor: usize, controlled: bool) -> bool {
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let frame = runtime.frame;
    let Some(session) = runtime
        .session
        .as_mut()
        .filter(|session| session.actor() == actor && unsafe { session.valid(state) })
    else {
        return controlled;
    };
    if session.debug.mode() == RunMode::Paused || session.pumped_frame == Some(frame) {
        session.wake = true;
        return true;
    }
    false
}

unsafe fn active_cursor(actor: usize) -> u32 {
    let lane = unsafe { get::<u16>(actor + 3288) };
    let offset = if lane & 0x20 != 0 {
        2604
    } else if lane & 0x10 != 0 {
        2596
    } else if lane & 1 != 0 {
        2588
    } else {
        2548
    };
    unsafe { get(actor + offset) }
}

unsafe fn enter(state: &State, registers: &mut hooks::Registers, floating: &mut [u8; 512]) -> bool {
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(session) = runtime
        .session
        .as_mut()
        .filter(|s| s.actor() == registers.esi as usize)
    else {
        return false;
    };
    if !unsafe { session.valid(state) } {
        return false;
    }
    session.entered = true;
    session.wake = false;
    if let Some(resume) = session.resume.take() {
        if resume.control == unsafe { ControlStamp::read(session.actor()) } {
            unsafe { registers.restore(&resume.continuation, floating) };
            true
        } else {
            session.debug.invalidate("原生控制状态已变化，请重新附加");
            session.cached.take();
            false
        }
    } else {
        false
    }
}

unsafe fn instruction(
    state: &State,
    registers: &mut hooks::Registers,
    floating: &[u8; 512],
) -> bool {
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let frame = runtime.frame;
    let Some(session) = runtime
        .session
        .as_mut()
        .filter(|s| s.entered && s.actor() == registers.esi as usize)
    else {
        return false;
    };
    if session.detaching {
        return false;
    }
    let count = unsafe { registers.count() };
    let native_exit =
        count >= 999 || registers.edi == 0 || unsafe { get::<u8>(session.actor() + 3182) } != 0;
    if native_exit {
        return false;
    }
    let snapshot = unsafe { session.capture(registers.edi, frame) };
    session.complete(snapshot, Outcome::Continue);
    match session.instruction_bytes(registers.edi) {
        Ok(bytes) => {
            if session.debug.before_instruction(&session.current, bytes[0]) {
                session.pending = Some(bytes);
                return false;
            }
        }
        Err(error) => {
            session.debug.invalidate(error.clone());
            session.reason = Some(error);
        }
    }
    session.resume = Some(ParkedTurn {
        continuation: unsafe { registers.save(floating) },
        control: unsafe { ControlStamp::read(session.actor()) },
    });
    registers.eax = 0;
    true
}

unsafe fn leave(state: &State, registers: &hooks::Registers) {
    let mut runtime = state
        .ai_debug
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let frame = runtime.frame;
    let Some(session) = runtime
        .session
        .as_mut()
        .filter(|s| s.entered && s.actor() == registers.esi as usize)
    else {
        return;
    };
    session.entered = false;
    if session.resume.is_some() {
        return;
    }
    let outcome = if session
        .pending
        .as_ref()
        .is_some_and(|bytes| bytecode::is_stop(bytes[0]))
    {
        Outcome::Reset
    } else {
        Outcome::Yield
    };
    let cursor = unsafe { active_cursor(session.actor()) };
    let after = unsafe { session.capture(cursor, frame) };
    session.complete(after, outcome);
}
