use crate::primitives::focus::{focus_on_click, navigation_allowed, scroll_on_focus};
use crate::primitives::layout::{ListOutput, VirtualList, scroll_keyboard};
use crate::theme::{Tokens, paint};
use crate::{Panel, Surface};
use egui::{
    Id, InnerResponse, Response, ScrollArea, Sense, Shape, Ui, UiBuilder,
    scroll_area::ScrollAreaOutput,
};

/// A styled Panel containing a native ScrollArea and optional list navigation.
#[must_use = "Call show, show_rows or show_list to render the panel"]
pub struct ScrollPanel<'a> {
    id: Id,
    panel: Panel<'a>,
    /// Configure native scrollbars, sizing, offsets and scrolling sources here.
    pub scroll: ScrollArea,
}

impl<'a> ScrollPanel<'a> {
    pub fn new(id: Id, title: &'a str) -> Self {
        Self {
            id,
            panel: Panel::new(title),
            scroll: ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, true]),
        }
    }

    pub fn surface(mut self, surface: Surface) -> Self {
        self.panel = self.panel.surface(surface);
        self
    }
    /// The returned response is the keyboard-scroll viewport. The panel title
    /// and padding stay decorative; interactive children keep their own focus.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<ScrollAreaOutput<R>> {
        self.show_content(ui, |ui, scroll, focus| {
            scroll.show(ui, |ui| {
                let inner = content(ui);
                scroll_keyboard(ui, focus);
                inner
            })
        })
    }
    /// Virtualized content with the same viewport focus behavior as `show`.
    pub fn show_rows<R>(
        self,
        ui: &mut Ui,
        row_height: f32,
        count: usize,
        content: impl FnOnce(&mut Ui, std::ops::Range<usize>) -> R,
    ) -> InnerResponse<ScrollAreaOutput<R>> {
        self.show_content(ui, |ui, scroll, focus| {
            scroll.show_rows(ui, row_height, count, |ui, rows| {
                let inner = content(ui, rows);
                scroll_keyboard(ui, focus);
                inner
            })
        })
    }
    /// A decorative panel containing one native list control. Tab reaches the
    /// list directly; rows handle navigation and value activation. The outer
    /// response describes the panel and `inner.response` is the focus target.
    pub fn show_list(
        self,
        ui: &mut Ui,
        row_height: f32,
        count: usize,
        enabled: impl Fn(usize) -> bool,
        mut row: impl FnMut(&mut Ui, usize) -> Response,
    ) -> InnerResponse<ListOutput> {
        self.panel.show(ui, |ui| {
            VirtualList::new(self.id).show(
                ui,
                self.scroll,
                row_height,
                count,
                enabled,
                |ui, index, current| paint::active_part(ui, current, |ui| row(ui, index)).inner,
            )
        })
    }
    fn show_content<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, ScrollArea, Id) -> ScrollAreaOutput<R>,
    ) -> InnerResponse<ScrollAreaOutput<R>> {
        let id = ui.make_persistent_id(("scroll-panel-focus", self.id));
        let output = self
            .panel
            .show(ui, |ui| {
                let focused = navigation_allowed(ui) && ui.memory(|memory| memory.has_focus(id));
                let visuals = ui.visuals().widgets.active;
                let fill = if focused {
                    visuals.bg_fill
                } else {
                    ui.stack().bg_color()
                };
                // Native scroll edge effects read their nearest frame's fill,
                // including this viewport's focused background.
                let info = egui::UiStackInfo::default().with_frame(egui::Frame::new().fill(fill));
                let background = ui.painter().add(Shape::Noop);
                let output = ui.scope_builder(
                    UiBuilder::new()
                        .id(id)
                        .sense(Sense::click())
                        .ui_stack_info(info),
                    |ui| content(ui, self.scroll, id),
                );
                if focused {
                    ui.painter().set(
                        background,
                        paint::chamfer(
                            output.inner.inner_rect.shrink(0.5),
                            Tokens::get(ui).cut,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                        ),
                    );
                }
                output
            })
            .inner;
        focus_on_click(&output.response);
        scroll_on_focus(&output.response);
        output
    }
}
