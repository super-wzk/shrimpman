//! Hunter-guild styling and widgets for any egui host.
//!
//! Install [`Theme`] once, then compose widgets with normal egui layouts:
//! ```
//! # egui::__run_test_ui(|ui| {
//! use egui_hunter::{ButtonKind, Icon, Theme};
//! let theme = Theme::default();
//! theme.panel("任务列表").show(ui, |ui| {
//!     ui.add(theme.button("接受任务").kind(ButtonKind::Primary).icon(Icon::Quest));
//! });
//! # });
//! ```
//! Fonts, textures, application state and input routing belong to the host.
//! All sizes use egui points; egui handles DPI scaling and clipping.

#![forbid(unsafe_code)]

mod containers;
mod fields;
mod icons;
mod information;
mod input;
mod layout;
mod list;
mod navigation;
mod notifications;
mod paint;
mod panel;
mod theme;
mod widgets;

pub use containers::{Dialog, Popup, ScrollPanel, Window};
pub use fields::{TextField, Validation};
pub use icons::Icon;
pub use information::{Meter, NoticeKind, Property, RichTooltip};
pub use input::{Direction, GamepadState, InputDevice, NavigationInput};
pub use layout::{ResponsiveColumns, Tab, Tabs, TabsState};
pub use navigation::{FocusGroup, MenuStack, OverlayState};
pub use notifications::Notifications;
pub use panel::{Panel, Surface};
pub use theme::{Metrics, Palette, Theme};
pub use widgets::{Button, ButtonKind, Checkbox, ItemSlot, Toggle};
