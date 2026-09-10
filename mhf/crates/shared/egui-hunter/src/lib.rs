//! Hunter-guild styling and widgets for any egui host.
//!
//! Install [`Theme`] once, then compose widgets with normal egui layouts:
//! ```
//! # egui::__run_test_ui(|ui| {
//! use egui_hunter::{Button, ButtonKind, Icon, Panel};
//! Panel::new("任务列表").show(ui, |ui| {
//!     ui.add(Button::new("接受任务").kind(ButtonKind::Primary).icon(Icon::Quest));
//! });
//! # });
//! ```
//! Fonts, textures, application state and input routing belong to the host.
//! All sizes use egui points; egui handles DPI scaling and clipping.

#![forbid(unsafe_code)]

pub mod components;
pub mod input;
pub mod primitives;
pub mod theme;

pub use components::{
    Button, ButtonKind, Checkbox, Dialog, DialogState, Field, FormLayout, ItemSlot, LabelPlacement,
    Meter, NoticeKind, Notifications, Panel, Popup, Property, RichTooltip, ScrollPanel,
    SelectField, Surface, Tab, Tabs, TextField, Toggle, Validation, Window, key_hint, notice,
    properties,
};
pub use input::{Direction, GamepadState, InputDevice, NavigationInput, consume_escape};
pub use primitives::focus::{EngagementPlugin, FocusEngagement, FocusGroup, scroll_on_focus};
pub use primitives::layout::{ListOutput, ResponsiveColumns, VirtualList, scroll_keyboard};
pub use primitives::navigation::{NavigationStack, NavigationState};
pub use theme::{Icon, Theme, Tokens};
