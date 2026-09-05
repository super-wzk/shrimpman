use egui::{Context, Id, InnerResponse, Key, Modifiers, Response, Ui, Vec2};

use crate::{OverlayState, Panel, Theme};

/// Floating egui window with the same panel frame as the rest of the library.
/// Native egui owns movement, edge resizing, bounds and layer ordering.
#[must_use = "Call show to render the window"]
pub struct Window<'a> {
    theme: &'a Theme,
    title: &'a str,
    native: egui::Window<'a>,
    open: Option<&'a mut bool>,
}

impl Theme {
    pub fn window<'a>(&'a self, title: &'a str) -> Window<'a> {
        Window {
            theme: self,
            title,
            native: egui::Window::new(title)
                .frame(egui::Frame::NONE)
                .title_bar(false)
                .collapsible(false)
                .fade_out(false),
            open: None,
        }
    }

    pub fn scroll_panel<'a>(&'a self, id: Id, title: &'a str) -> ScrollPanel<'a> {
        ScrollPanel {
            theme: self,
            focus_id: id,
            panel: self.panel(title),
            scroll: egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, true]),
        }
    }

    pub fn dialog<'a>(&'a self, id: Id, title: &'a str) -> Dialog<'a> {
        Dialog {
            theme: self,
            id,
            title,
            width: 390.0,
            initial_focus: None,
            dismiss_on_backdrop: true,
        }
    }

    /// Anchored popup. `show` toggles its state when this anchor is clicked.
    pub fn popup<'a>(&'a self, anchor: &'a Response) -> Popup<'a> {
        Popup {
            theme: self,
            anchor,
            title: "",
            width: 240.0,
            initial_focus: None,
        }
    }
}

impl<'a> Window<'a> {
    pub fn id(mut self, id: Id) -> Self {
        self.native = self.native.id(id);
        self
    }
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }
    pub fn default_size(mut self, size: impl Into<Vec2>) -> Self {
        self.native = self.native.default_size(size);
        self
    }
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.native = self.native.resizable(resizable);
        self
    }
    pub fn movable(mut self, movable: bool) -> Self {
        self.native = self.native.movable(movable);
        self
    }
    pub fn default_pos(mut self, pos: impl Into<egui::Pos2>) -> Self {
        self.native = self.native.default_pos(pos);
        self
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        let closable = self.open.is_some();
        let mut native = self.native;
        if let Some(value) = self.open {
            native = native.open(value);
        }
        let panel = self.theme.panel(self.title);
        let close_button = self.theme.button("关闭").min_size(egui::vec2(60.0, 28.0));
        let response = native.show(ctx, |ui| {
            let result = ui.scope_builder(egui::UiBuilder::new().closable(), |ui| {
                panel
                    .show_with_header(
                        ui,
                        |ui| {
                            if closable && ui.add(close_button).clicked() {
                                ui.close();
                            }
                        },
                        |ui| {
                            // Fill the native resize area so the frame follows both edges.
                            ui.set_min_height(ui.available_height());
                            content(ui)
                        },
                    )
                    .inner
            });
            // Native Window still contains a Collapsible even when its
            // title bar is hidden. Give ui.close() a direct window target.
            if result.response.should_close() {
                ui.close_kind(egui::UiKind::Window);
            }
            result.inner
        })?;
        Some(InnerResponse::new(response.inner?, response.response))
    }
}

/// A stationary title/frame around native scrolling content. Use `show_rows`
/// for equal-height content or `show_list` for focusable rows with navigation.
/// Both virtualize large lists and expose the scroll offset and viewport.
#[must_use = "Call show, show_rows or show_list to render the scroll panel"]
pub struct ScrollPanel<'a> {
    theme: &'a Theme,
    focus_id: Id,
    panel: Panel<'a>,
    scroll: egui::ScrollArea,
}

impl ScrollPanel<'_> {
    pub fn max_height(mut self, height: f32) -> Self {
        self.scroll = self.scroll.max_height(height.max(0.0));
        self
    }
    pub fn offset(mut self, offset: Vec2) -> Self {
        self.scroll = self.scroll.scroll_offset(offset);
        self
    }
    pub fn surface(mut self, surface: crate::Surface) -> Self {
        self.panel = self.panel.surface(surface);
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<egui::scroll_area::ScrollAreaOutput<R>> {
        self.panel.show(ui, |ui| {
            self.scroll.show(ui, |ui| {
                let result = content(ui);
                self.theme.scroll_focus(ui, self.focus_id);
                result
            })
        })
    }

    /// `row_height` excludes the gap between rows, matching egui's `show_rows`.
    pub fn show_rows(
        self,
        ui: &mut Ui,
        row_height: f32,
        total_rows: usize,
        content: impl FnOnce(&mut Ui, std::ops::Range<usize>),
    ) -> InnerResponse<egui::scroll_area::ScrollAreaOutput<()>> {
        self.panel.show(ui, |ui| {
            self.scroll
                .show_rows(ui, row_height, total_rows, |ui, rows| {
                    content(ui, rows);
                    self.theme.scroll_focus(ui, self.focus_id);
                })
        })
    }

    /// Virtualized single-control rows with logical Up/Down, PageUp/PageDown and
    /// Home/End navigation. Tab leaves the list in the native focus order.
    /// `enabled` is queried without rendering rows, so disabled/offscreen rows
    /// can be skipped. Return one focusable response of `row_height` per row.
    /// Indices identify stable positions; give controls stable IDs when reordering.
    pub fn show_list(
        self,
        ui: &mut Ui,
        row_height: f32,
        total_rows: usize,
        enabled: impl Fn(usize) -> bool,
        mut row_content: impl FnMut(&mut Ui, usize) -> Response,
    ) -> InnerResponse<egui::scroll_area::ScrollAreaOutput<()>> {
        self.panel.show(ui, |ui| {
            let id = ui.make_persistent_id(("list-navigation", self.focus_id));
            let mut navigation = ui.data_mut(|data| {
                data.get_temp::<crate::list::ListNavigation>(id)
                    .unwrap_or_default()
            });
            let mut scroll = self.scroll.animated(false);
            if let Some(offset) = navigation.prepare(ui, row_height, total_rows, &enabled) {
                scroll = scroll.vertical_scroll_offset(offset);
            }
            let output = scroll.show_rows(ui, row_height, total_rows, |ui, rows| {
                for row in rows {
                    let response = ui
                        .add_enabled_ui(enabled(row), |ui| row_content(ui, row))
                        .inner;
                    navigation.observe(ui, row, &response);
                }
                self.theme.scroll_focus(ui, self.focus_id);
            });
            navigation.offset = output.state.offset.y;
            navigation.viewport_height = output.inner_rect.height();
            ui.data_mut(|data| data.insert_temp(id, navigation));
            output
        })
    }
}

#[must_use = "Call show each frame, even when closed, to restore focus"]
pub struct Dialog<'a> {
    theme: &'a Theme,
    id: Id,
    title: &'a str,
    width: f32,
    initial_focus: Option<Id>,
    dismiss_on_backdrop: bool,
}

impl Dialog<'_> {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(120.0);
        self
    }
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.initial_focus = Some(id);
        self
    }
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        state: &mut OverlayState,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        state.prepare(ctx);
        if !state.open {
            return None;
        }
        let modal = egui::Modal::new(self.id)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.set_width(
                    self.width
                        .min((ctx.content_rect().width() - 32.0).max(120.0)),
                );
                self.theme.panel(self.title).show(ui, content).inner
            });
        let close = modal.response.should_close()
            || (self.dismiss_on_backdrop && modal.backdrop_response.clicked())
            || (modal.is_top_modal
                && !modal.any_popup_open
                && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)));
        if close {
            state.close(ctx);
        } else if state.just_opened
            && !egui::Popup::is_any_open(ctx)
            && let Some(id) = self.initial_focus
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        state.just_opened = false;
        Some(InnerResponse::new(modal.inner, modal.response))
    }
}

#[must_use = "Call show to render the popup"]
pub struct Popup<'a> {
    theme: &'a Theme,
    anchor: &'a Response,
    title: &'a str,
    width: f32,
    initial_focus: Option<Id>,
}

impl<'a> Popup<'a> {
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(80.0);
        self
    }
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.initial_focus = Some(id);
        self
    }

    pub fn show<R>(
        self,
        state: &mut OverlayState,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        let ctx = &self.anchor.ctx;
        state.prepare(ctx);
        if self.anchor.clicked() {
            state.toggle_from(self.anchor);
        }
        if !state.open {
            return None;
        }
        let id = self.anchor.id.with("hunter-popup");
        state.popup_id = Some(id);
        let command = state
            .just_opened
            .then_some(egui::SetOpenCommand::Bool(true));
        let output = egui::Popup::from_response(self.anchor)
            .id(id)
            .open_memory(command)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(egui::Frame::NONE)
            .gap(self.theme.metrics.gap)
            .width(self.width)
            .show(|ui| {
                ui.set_width(
                    self.width
                        .min((ctx.content_rect().width() - 32.0).max(80.0)),
                );
                self.theme.panel(self.title).show(ui, content).inner
            });
        if !egui::Popup::is_id_open(ctx, id) {
            let another_popup_open = egui::Popup::is_any_open(ctx);
            // Outside clicks may focus a different control. A replacement popup
            // owns focus too; neither case should return it to the old anchor.
            let restore = output
                .as_ref()
                .is_some_and(|response| !response.response.clicked_elsewhere())
                && !another_popup_open;
            state.close_with_focus(ctx, restore);
            // Native Popup observes Escape without consuming it. Stop that same
            // key from reaching a parent dialog or menu later in this frame.
            if output.is_some() && !another_popup_open {
                ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            }
        } else if state.just_opened
            && let Some(id) = self.initial_focus
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        state.just_opened = false;
        output
    }
}
