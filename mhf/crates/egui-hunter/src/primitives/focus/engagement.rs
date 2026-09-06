//! Optional controller focus engagement. Keyboard and pointer input stay native.
use std::collections::HashMap;

use egui::{
    Context, Event, Id, InnerResponse, Key, LayerId, RawInput, Rect, Response, Sense, Ui,
    UiBuilder, ViewportId,
};

use crate::input::{Direction, InputDevice, consume_escape};

const REGION_TAG: &str = "hunter-engagement-region";

#[derive(Clone, Copy)]
struct RegionTag {
    scope: Id,
    region: Id,
}

/// Optional egui lifecycle storage for explicitly declared [`FocusEngagement`]
/// regions. Install once before the host's first `NavigationInput::apply`.
/// Engagement is scoped to the root viewport used by a game's UI. Other
/// native viewports keep the ordinary adapter and widget behavior. This plugin
/// stores IDs and geometry, never a Context or Response.
#[derive(Default)]
pub struct EngagementPlugin {
    state: ControllerState,
    root_memory_current: bool,
    ended_root_pass: bool,
}

#[derive(Clone, Default)]
struct ControllerState {
    device: InputDevice,
    pass: u64,
    order: u64,
    scopes: HashMap<Id, Scope>,
    staged: ControllerInput,
    input: ControllerInput,
    injected: Vec<(usize, Event)>,
}

#[derive(Clone, Default)]
struct Scope {
    layer: Option<LayerId>,
    selected: Option<Id>,
    engaged: bool,
    initial: bool,
    remembered: HashMap<Id, Id>,
    regions: Vec<Region>,
    reveal: Option<Id>,
    seen: u64,
    order: u64,
}

#[derive(Clone)]
struct Region {
    id: Id,
    rect: Rect,
    enabled: bool,
    controls: Vec<Control>,
}
#[derive(Clone)]
struct Control {
    id: Id,
    rect: Rect,
    enabled: bool,
}

#[derive(Clone, Default)]
struct ControllerInput {
    gamepad: bool,
    owner: Option<RegionTag>,
    in_scope: bool,
    popup_open: bool,
    cancel: bool,
    editor_cancel: bool,
    direction: Option<Direction>,
    direction_origin: Option<Id>,
    constrain_direction: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ControllerAction {
    Direction(Direction, bool),
    Confirm,
    Cancel,
    Next(bool),
}

#[derive(Clone, Copy)]
pub(crate) struct Forward {
    pub key: Key,
    pub repeat: bool,
    owner: Option<RegionTag>,
    in_scope: bool,
    cancel: bool,
    editor_cancel: bool,
    direction: Option<Direction>,
    direction_origin: Option<Id>,
}

pub(crate) enum Route {
    Handled,
    Forward(Forward),
}

impl egui::Plugin for EngagementPlugin {
    fn debug_name(&self) -> &'static str {
        "Focus Engagement"
    }

    fn input_hook(&mut self, _ctx: &Context, raw: &mut RawInput) {
        if raw.viewport_id != ViewportId::ROOT {
            return;
        }
        let state = &mut self.state;
        let physical = raw.events.iter().enumerate().any(|(index, event)| {
            !state
                .injected
                .iter()
                .any(|(at, own)| *at == index && own == event)
                && native_activity(event)
        });
        if physical {
            // If another host hook appended physical input after the adapter,
            // discard only our own known pulses. Never consume that input.
            let mut index = 0;
            raw.events.retain(|event| {
                let ours = state
                    .injected
                    .iter()
                    .any(|(at, own)| *at == index && own == event);
                index += 1;
                !ours
            });
            state.device = InputDevice::KeyboardMouse;
            state.input = ControllerInput::default();
            for scope in state.scopes.values_mut() {
                scope.engaged = false;
            }
        } else {
            state.input = std::mem::take(&mut state.staged);
            if state.input.gamepad {
                state.device = InputDevice::Gamepad;
            }
        }
        state.staged = ControllerInput::default();
        state.injected.clear();
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        self.root_memory_current = ui.ctx().viewport_id() == ViewportId::ROOT;
        if self.root_memory_current {
            self.state.pass += 1;
        }
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        let id = ui.ctx().viewport_id();
        self.ended_root_pass = id == ViewportId::ROOT;
        self.root_memory_current = id == ViewportId::ROOT;
        if self.root_memory_current {
            let state = &mut self.state;
            state.scopes.retain(|_, scope| scope.seen == state.pass);
        }
    }

    fn output_hook(&mut self, ctx: &Context, _: &mut egui::FullOutput) {
        if !std::mem::take(&mut self.ended_root_pass) || !self.root_memory_current {
            return;
        }
        let state = &mut self.state;
        let Some(direction) = state.input.direction.take() else {
            return;
        };
        let Some(owner) = state.input.owner else {
            return;
        };
        if state.device != InputDevice::Gamepad
            || !state.input.constrain_direction
            || state.input.popup_open
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let Some(scope) = state.scopes.get_mut(&owner.scope) else {
            return;
        };
        if !scope
            .layer
            .is_some_and(|layer| ctx.memory(|memory| memory.allows_interaction(layer)))
        {
            return;
        }
        let Some(region) = scope
            .regions
            .iter()
            .find(|region| region.id == owner.region && region.available())
        else {
            return;
        };
        let focused = ctx.memory(|memory| memory.focused());
        if region
            .controls
            .iter()
            .any(|control| control.enabled && Some(control.id) == focused)
        {
            if let Some(focused) = focused {
                scope.remembered.insert(region.id, focused);
            }
            return;
        }
        let Some(origin) = state.input.direction_origin else {
            return;
        };
        if !region
            .controls
            .iter()
            .any(|control| control.id == origin && control.enabled)
        {
            return;
        }
        // Native editors/sliders can handle arrows without consuming their
        // Event. Let their native focus filter win; repair only an actual
        // out-of-region move caused by this controller pulse after end_pass.
        let target = spatial(
            region
                .controls
                .iter()
                .filter(|control| control.enabled)
                .map(|control| (control.id, control.rect)),
            origin,
            direction,
        )
        .unwrap_or(origin);
        if focused != Some(target) {
            ctx.memory_mut(|memory| memory.request_focus(target));
            scope.remembered.insert(region.id, target);
            scope.reveal = Some(target);
            ctx.request_repaint();
        }
    }
}

/// Explicit page regions for controller engagement. All child widgets remain
/// enabled and keyboard/mouse input remains egui's ordinary widget behavior.
/// `begin` and `navigate` surround the page's `show` calls.
pub struct FocusEngagement {
    id: Id,
    scope: Scope,
    controller: bool,
    blocked: bool,
}

impl FocusEngagement {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            scope: Scope::default(),
            controller: false,
            blocked: false,
        }
    }

    /// `initial_region` selects only the controller's initial frame. The page
    /// separately chooses its normal initial keyboard control.
    pub fn begin(&mut self, ui: &Ui, initial_region: Option<Id>) {
        if ui.ctx().viewport_id() != ViewportId::ROOT {
            self.scope = Scope::default();
            self.controller = false;
            self.blocked = true;
            return;
        }
        drop(ui.ctx().plugin_or_default::<EngagementPlugin>());
        let (scope, device) = ui
            .ctx()
            .with_plugin::<EngagementPlugin, _>(|plugin| {
                let state = &mut plugin.state;
                (
                    state.scopes.remove(&self.id).unwrap_or_default(),
                    state.device,
                )
            })
            .unwrap();
        self.scope = scope;
        self.scope.regions.clear();
        self.controller = device == InputDevice::Gamepad;
        self.blocked = !ui.is_enabled()
            || !ui.memory(|memory| memory.allows_interaction(ui.layer_id()))
            || egui::Popup::is_any_open(ui.ctx());
        self.scope.layer = Some(ui.layer_id());
        if let Some(initial) = initial_region {
            self.scope.selected = Some(initial);
            self.scope.engaged = false;
            self.scope.initial = self.controller;
        }
    }

    /// Declare a region around presentation UI, then register its actual
    /// native focusable controls. This does not disable or restyle children.
    pub fn show<R>(
        &mut self,
        ui: &mut Ui,
        id: Id,
        content: impl FnOnce(&mut Ui, &mut Vec<Response>) -> R,
    ) -> InnerResponse<R> {
        debug_assert!(self.scope.regions.iter().all(|region| region.id != id));
        let frame_target = self.controller
            && !self.blocked
            && !self.scope.engaged
            && self.scope.selected == Some(id);
        let sense = if frame_target {
            Sense::click()
        } else {
            Sense::CLICK
        };
        let info = egui::UiStackInfo::default().with_tag_value(
            REGION_TAG,
            RegionTag {
                scope: self.id,
                region: id,
            },
        );
        let mut controls = Vec::new();
        let output = ui.scope_builder(
            UiBuilder::new().id(id).sense(sense).ui_stack_info(info),
            |ui| content(ui, &mut controls),
        );
        let region = Region {
            id,
            rect: output.response.rect,
            enabled: output.response.enabled(),
            controls: controls
                .iter()
                .filter(|response| response.sense.is_focusable())
                .map(|response| Control {
                    id: response.id,
                    rect: response.rect,
                    enabled: response.enabled(),
                })
                .collect(),
        };
        if let Some(control) = controls
            .iter()
            .find(|control| control.has_focus() && control.enabled())
        {
            self.scope.remembered.insert(id, control.id);
        }
        if !self.controller && output.response.clicked_by(egui::PointerButton::Primary) {
            self.scope.selected = Some(id);
        }
        if self.controller && !self.blocked {
            if self.scope.reveal == Some(id) && output.response.has_focus() {
                output.response.scroll_to_me(Some(egui::Align::TOP));
                self.scope.reveal = None;
            } else if let Some(control) = controls
                .iter()
                .find(|control| self.scope.reveal == Some(control.id) && control.has_focus())
            {
                control.scroll_to_me(None);
                self.scope.reveal = None;
            }
        }
        self.blocked |= egui::Popup::is_any_open(ui.ctx());
        self.scope.regions.push(region);
        output
    }

    /// Finish after child UI has handled its input. Only controller-originated
    /// Back can disengage here; physical Escape is never consumed.
    pub fn navigate(&mut self, ui: &Ui) -> Option<Id> {
        if ui.ctx().viewport_id() != ViewportId::ROOT {
            return None;
        }
        self.blocked |= egui::Popup::is_any_open(ui.ctx())
            || !ui.memory(|memory| memory.allows_interaction(ui.layer_id()));
        let input = ui
            .ctx()
            .with_plugin::<EngagementPlugin, _>(|plugin| plugin.state.input.clone())
            .unwrap_or_default();
        let focused = ui.memory(|memory| memory.focused());
        let owned = input.owner.is_some_and(|owner| owner.scope == self.id);
        let mut target = None;
        if self.controller && !self.blocked {
            if self.scope.initial {
                target = self.scope.frame_target();
                if target.is_some() {
                    self.scope.initial = false;
                }
            } else if let Some(selected) = self.scope.selected {
                let region = self
                    .scope
                    .regions
                    .iter()
                    .find(|region| region.id == selected && region.available());
                if let Some(region) = region {
                    if self.scope.engaged {
                        let remembered = self.scope.remembered.get(&selected).copied();
                        if remembered.is_some()
                            && !region
                                .controls
                                .iter()
                                .any(|control| control.enabled && Some(control.id) == remembered)
                        {
                            target = self.scope.child_target(selected);
                        }
                        // Preserve a deliberately blurred editor and ordinary
                        // native transient no-focus states; do not reopen them.
                    }
                } else {
                    self.scope.engaged = false;
                    self.scope.selected = self
                        .scope
                        .regions
                        .iter()
                        .find(|region| region.available())
                        .map(|region| region.id);
                    target = self.scope.selected;
                }
            }
            if owned
                && input.cancel
                && self.scope.engaged
                && consume_escape(ui.ctx())
                && !input.editor_cancel
            {
                self.scope.engaged = false;
                target = self.scope.selected;
            }
            if let Some(target) = target {
                self.request(ui.ctx(), target);
            }
        }
        if !self.controller {
            self.scope.reveal = None;
            if let Some(region) = self.scope.regions.iter().find(|region| {
                region
                    .controls
                    .iter()
                    .any(|control| Some(control.id) == focused)
            }) {
                self.scope.selected = Some(region.id);
                if let Some(focused) = focused {
                    self.scope.remembered.insert(region.id, focused);
                }
            }
        }
        // A callback that deliberately focuses another window/control wins.
        // Native directional focus navigation happens later, at end_pass.
        let constrain = owned
            && !self.blocked
            && self.scope.engaged
            && input.direction.is_some()
            && self.scope.selected.is_some_and(|selected| {
                self.scope
                    .regions
                    .iter()
                    .find(|region| region.id == selected)
                    .is_some_and(|region| {
                        region
                            .controls
                            .iter()
                            .any(|control| control.enabled && Some(control.id) == focused)
                    })
            });
        self.scope
            .remembered
            .retain(|id, _| self.scope.regions.iter().any(|region| region.id == *id));
        ui.ctx().with_plugin::<EngagementPlugin, _>(|plugin| {
            let state = &mut plugin.state;
            state.order += 1;
            self.scope.order = state.order;
            self.scope.seen = state.pass;
            state
                .scopes
                .insert(self.id, std::mem::take(&mut self.scope));
            if owned {
                state.input.cancel = false;
                state.input.constrain_direction = constrain;
            }
        });
        target
    }

    fn request(&mut self, ctx: &Context, id: Id) {
        if ctx.memory(|memory| memory.focused()) != Some(id) {
            ctx.memory_mut(|memory| memory.request_focus(id));
            self.scope.reveal = Some(id);
            ctx.request_repaint();
        }
    }
}

impl Region {
    fn available(&self) -> bool {
        self.enabled && self.controls.iter().any(|control| control.enabled)
    }
}
impl Scope {
    fn frame_target(&mut self) -> Option<Id> {
        let target = self
            .selected
            .filter(|id| {
                self.regions
                    .iter()
                    .any(|region| region.id == *id && region.available())
            })
            .or_else(|| {
                self.regions
                    .iter()
                    .find(|region| region.available())
                    .map(|region| region.id)
            });
        self.selected = target;
        target
    }
    fn child_target(&self, region: Id) -> Option<Id> {
        let region = self
            .regions
            .iter()
            .find(|item| item.id == region && item.available())?;
        region
            .controls
            .iter()
            .find(|control| control.enabled && Some(&control.id) == self.remembered.get(&region.id))
            .or_else(|| region.controls.iter().find(|control| control.enabled))
            .map(|control| control.id)
    }
}

pub(crate) fn prepare_input(ctx: &Context, viewport_id: ViewportId) {
    if viewport_id != ViewportId::ROOT {
        return;
    }
    ctx.with_plugin::<EngagementPlugin, _>(|plugin| {
        let state = &mut plugin.state;
        state.staged = ControllerInput::default();
        state.injected.clear();
    });
}

pub(crate) fn route(
    ctx: &Context,
    viewport_id: ViewportId,
    action: ControllerAction,
) -> Option<Route> {
    if viewport_id != ViewportId::ROOT {
        return None;
    }
    let mut state = ctx.with_plugin::<EngagementPlugin, _>(|plugin| {
        plugin.root_memory_current.then(|| plugin.state.clone())
    })??;
    let previous_device = state.device;
    state.device = InputDevice::Gamepad;
    let focused = ctx.memory(|memory| memory.focused());
    let eligible = |scope: &&Scope| {
        scope.regions.iter().any(Region::available)
            && scope
                .layer
                .is_some_and(|layer| ctx.memory(|memory| memory.allows_interaction(layer)))
    };
    let scope_id = if egui::Popup::is_any_open(ctx) {
        None
    } else {
        state
            .scopes
            .iter()
            .filter(|(_, scope)| eligible(scope))
            .find(|(_, scope)| {
                scope.regions.iter().any(|region| {
                    Some(region.id) == focused
                        || region
                            .controls
                            .iter()
                            .any(|control| Some(control.id) == focused)
                })
            })
            .map(|(id, _)| *id)
            .or_else(|| {
                focused
                    .is_none()
                    .then(|| {
                        state
                            .scopes
                            .iter()
                            .filter(|(_, scope)| eligible(scope))
                            .max_by_key(|(_, scope)| scope.order)
                            .map(|(id, _)| *id)
                    })
                    .flatten()
            })
    };
    let mut request = None;
    let result = scope_id.map(|id| {
        let scope = state.scopes.get_mut(&id).unwrap();
        let current_child = scope
            .regions
            .iter()
            .find(|region| {
                region
                    .controls
                    .iter()
                    .any(|control| control.enabled && Some(control.id) == focused)
            })
            .map(|region| region.id);
        if previous_device != InputDevice::Gamepad {
            scope.selected = current_child.or(scope.selected);
            scope.engaged = false;
        }
        if matches!(action, ControllerAction::Cancel) && current_child.is_some() {
            scope.selected = current_child;
            scope.engaged = true;
        }
        if let Some(region) = current_child
            && let Some(focused) = focused
        {
            scope.remembered.insert(region, focused);
        }
        let Some(region_id) = scope.frame_target() else {
            return Route::Handled;
        };
        let mut forward = Forward {
            key: Key::Enter,
            repeat: false,
            owner: scope.engaged.then_some(RegionTag {
                scope: id,
                region: region_id,
            }),
            in_scope: true,
            cancel: false,
            editor_cancel: false,
            direction: None,
            direction_origin: focused,
        };
        if scope.engaged {
            match action {
                ControllerAction::Next(next) => {
                    let region = scope
                        .regions
                        .iter()
                        .find(|region| region.id == region_id)
                        .unwrap();
                    let ids = region
                        .controls
                        .iter()
                        .filter(|control| control.enabled)
                        .map(|control| control.id)
                        .collect::<Vec<_>>();
                    request = cycle(
                        &ids,
                        focused.or(scope.remembered.get(&region_id).copied()),
                        next,
                    );
                    if let Some(target) = request {
                        scope.remembered.insert(region_id, target);
                    }
                    Route::Handled
                }
                ControllerAction::Direction(direction, repeat) => {
                    forward.key = direction.key();
                    forward.repeat = repeat;
                    forward.direction = Some(direction);
                    Route::Forward(forward)
                }
                ControllerAction::Confirm => Route::Forward(forward),
                ControllerAction::Cancel => {
                    forward.key = Key::Escape;
                    forward.cancel = true;
                    forward.editor_cancel = ctx.text_edit_focused();
                    Route::Forward(forward)
                }
            }
        } else {
            match action {
                ControllerAction::Confirm => {
                    request = scope.child_target(region_id);
                    scope.engaged = request.is_some();
                    if let Some(target) = request {
                        scope.remembered.insert(region_id, target);
                    }
                    Route::Handled
                }
                ControllerAction::Cancel => {
                    forward.key = Key::Escape;
                    Route::Forward(forward)
                }
                ControllerAction::Next(next) => {
                    let ids = scope
                        .regions
                        .iter()
                        .filter(|region| region.available())
                        .map(|region| region.id)
                        .collect::<Vec<_>>();
                    request = cycle(&ids, Some(region_id), next);
                    scope.selected = request;
                    Route::Handled
                }
                ControllerAction::Direction(direction, _) => {
                    request = Some(
                        spatial(
                            scope
                                .regions
                                .iter()
                                .filter(|region| region.available())
                                .map(|region| (region.id, region.rect)),
                            region_id,
                            direction,
                        )
                        .unwrap_or(region_id),
                    );
                    scope.selected = request;
                    Route::Handled
                }
            }
        }
    });
    if let Some(target) = request
        && focused != Some(target)
    {
        ctx.memory_mut(|memory| memory.request_focus(target));
        if let Some(scope) = scope_id.and_then(|id| state.scopes.get_mut(&id)) {
            scope.reveal = Some(target);
        }
        ctx.request_repaint();
    }
    ctx.with_plugin::<EngagementPlugin, _>(|plugin| {
        plugin.state = state;
    });
    result
}

pub(crate) fn record_pulse(
    ctx: &Context,
    viewport_id: ViewportId,
    start: usize,
    events: &[Event],
    forward: Option<Forward>,
) {
    if viewport_id != ViewportId::ROOT {
        return;
    }
    ctx.with_plugin::<EngagementPlugin, _>(|plugin| {
        let state = &mut plugin.state;
        state.injected.extend(
            events
                .iter()
                .enumerate()
                .map(|(offset, event)| (start + offset, event.clone())),
        );
        state.staged.gamepad = true;
        state.staged.popup_open |= egui::Popup::is_any_open(ctx);
        if let Some(forward) = forward {
            state.staged.owner = forward.owner;
            state.staged.in_scope = forward.in_scope;
            state.staged.cancel |= forward.cancel;
            state.staged.editor_cancel |= forward.editor_cancel;
            if forward.direction.is_some() {
                state.staged.direction = forward.direction;
                state.staged.direction_origin = forward.direction_origin;
            }
        }
    });
}

pub(crate) fn navigation_allowed(ui: &Ui) -> bool {
    if ui.ctx().viewport_id() != ViewportId::ROOT {
        return true;
    }
    ui.ctx()
        .with_plugin::<EngagementPlugin, _>(|plugin| {
            let state = &plugin.state;
            if !state.input.gamepad {
                return true;
            }
            if state.input.popup_open {
                return ui.layer_id().order == egui::Order::Foreground;
            }
            let tag = ui
                .stack()
                .iter()
                .find_map(|node| node.tags().get_downcast::<RegionTag>(REGION_TAG));
            let Some(tag) = tag else {
                return true;
            };
            if let Some(owner) = state.input.owner {
                owner.scope == tag.scope && owner.region == tag.region
            } else {
                !state.input.in_scope
            }
        })
        .unwrap_or(true)
}

pub(crate) fn native_activity(event: &Event) -> bool {
    matches!(
        event,
        Event::Key { pressed: true, .. }
            | Event::Text(_)
            | Event::Paste(_)
            | Event::Cut
            | Event::PointerButton { pressed: true, .. }
            | Event::MouseWheel { .. }
    )
}

fn cycle(ids: &[Id], current: Option<Id>, next: bool) -> Option<Id> {
    if ids.is_empty() {
        return None;
    }
    let at = current.and_then(|current| ids.iter().position(|id| *id == current));
    Some(
        ids[match (at, next) {
            (Some(at), true) => (at + 1) % ids.len(),
            (Some(at), false) => (at + ids.len() - 1) % ids.len(),
            (None, true) => 0,
            (None, false) => ids.len() - 1,
        }],
    )
}
fn spatial(
    items: impl Iterator<Item = (Id, Rect)>,
    origin: Id,
    direction: Direction,
) -> Option<Id> {
    let items = items.collect::<Vec<_>>();
    let from = items.iter().find(|(id, _)| *id == origin)?.1.center();
    items
        .into_iter()
        .filter(|(id, _)| *id != origin)
        .filter_map(|(id, rect)| {
            let delta = rect.center() - from;
            let (along, across) = match direction {
                Direction::Up => (-delta.y, delta.x),
                Direction::Right => (delta.x, delta.y),
                Direction::Down => (delta.y, delta.x),
                Direction::Left => (-delta.x, delta.y),
            };
            (along > 0.0).then_some((along * along + across * across * 2.0, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}
