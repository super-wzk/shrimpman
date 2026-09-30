use egui::{Align, CursorIcon, Id, Layout, Rect, Response, Sense, Ui, UiBuilder, Vec2};

/// Two independently clipped panes filling the remaining space in their parent.
/// Give each split a stable, globally unique ID. Content owns its scroll areas.
#[must_use = "Call show to render the panes"]
pub struct SplitPane {
    id: Id,
    axis: Axis,
    default_ratio: f32,
    min_sizes: [f32; 2],
}

/// Native responses and the content returned by a [`SplitPane`].
pub struct SplitPaneOutput<R> {
    pub inner: R,
    pub response: Response,
    pub first: Response,
    pub second: Response,
    pub divider: Response,
    /// Actual first-pane fraction, excluding the divider.
    pub ratio: f32,
}

impl SplitPane {
    /// Left and right panes, separated by a draggable vertical divider.
    pub fn horizontal(id: Id) -> Self {
        Self::new(id, Axis::Horizontal)
    }

    /// Top and bottom panes, separated by a draggable horizontal divider.
    pub fn vertical(id: Id) -> Self {
        Self::new(id, Axis::Vertical)
    }

    fn new(id: Id, axis: Axis) -> Self {
        Self {
            id,
            axis,
            default_ratio: 0.5,
            min_sizes: [0.0; 2],
        }
    }

    /// Initial first-pane fraction and the value restored by double-clicking.
    /// A remembered user adjustment takes precedence until reset.
    pub fn default_ratio(mut self, ratio: f32) -> Self {
        self.default_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Minimum lengths along the split axis, in logical points.
    /// When both cannot fit, shrink them proportionally without growing the parent.
    pub fn min_sizes(mut self, first: f32, second: f32) -> Self {
        self.min_sizes = [first.max(0.0), second.max(0.0)];
        self
    }

    /// Render both panes once in a single closure, so shared application state
    /// can be used sequentially without overlapping closure captures.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, &mut Ui) -> R,
    ) -> SplitPaneOutput<R> {
        // ScrollArea can offer an infinite extent; use the viewport as its bound.
        let size = ui
            .available_size_before_wrap()
            .min(ui.ctx().content_rect().size())
            .max(Vec2::ZERO);
        let (_, rect) = ui.allocate_space(size);
        let mut response = ui.interact(rect, self.id, Sense::hover());
        let length = self.axis.component(size);
        let gap = self
            .axis
            .component(ui.spacing().item_spacing)
            .max(6.0)
            .min(length);
        let available = length - gap;
        let state_id = self.id.with("split-state");
        let mut state = ui.data_mut(|data| {
            data.get_temp::<SplitState>(state_id).unwrap_or(SplitState {
                ratio: self.default_ratio,
                drag_start: 0.0,
            })
        });
        let first_size = self.first_size(available, state.ratio);
        let (_, divider_rect, _) = self.axis.rects(rect, first_size, gap);
        let mut divider = ui.interact(
            divider_rect,
            self.id.with("divider"),
            Sense::CLICK | Sense::DRAG,
        );
        let previous_ratio = state.ratio;
        if divider.double_clicked() {
            state.ratio = self.default_ratio;
        } else {
            if divider.drag_started() {
                state.drag_start = first_size;
            }
            if available > self.min_sizes.iter().sum::<f32>()
                && let Some(delta) = divider.total_drag_delta()
            {
                state.ratio = self.first_size(
                    available,
                    (state.drag_start + self.axis.component(delta)) / available,
                ) / available;
            }
        }
        if state.ratio != previous_ratio {
            divider.mark_changed();
            response.mark_changed();
            ui.ctx().request_repaint();
        }
        ui.data_mut(|data| data.insert_temp(state_id, state));
        let first_size = self.first_size(available, state.ratio);
        let (first_rect, divider_rect, second_rect) = self.axis.rects(rect, first_size, gap);
        divider = divider
            .with_new_rect(divider_rect)
            .on_hover_cursor(self.axis.cursor())
            .on_hover_text("拖动调整 · 双击重置");
        let stroke = if divider.hovered() || divider.dragged() {
            ui.visuals().selection.stroke
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        };
        match self.axis {
            Axis::Horizontal => {
                ui.painter()
                    .vline(divider_rect.center().x, divider_rect.y_range(), stroke);
            }
            Axis::Vertical => {
                ui.painter()
                    .hline(divider_rect.x_range(), divider_rect.center().y, stroke);
            }
        }
        let mut first = self.child_ui(ui, "first", first_rect);
        let mut second = self.child_ui(ui, "second", second_rect);
        let inner = content(&mut first, &mut second);
        SplitPaneOutput {
            inner,
            response,
            first: first.response().with_new_rect(first_rect),
            second: second.response().with_new_rect(second_rect),
            divider,
            ratio: if available > 0.0 {
                first_size / available
            } else {
                state.ratio
            },
        }
    }

    fn first_size(&self, available: f32, ratio: f32) -> f32 {
        let [first, second] = self.min_sizes;
        if first + second > available {
            available * (first / (first + second))
        } else {
            (available * ratio).clamp(first, available - second)
        }
    }

    fn child_ui(&self, ui: &mut Ui, name: &str, rect: Rect) -> Ui {
        let mut child = ui.new_child(
            UiBuilder::new()
                .id(self.id.with(name))
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.shrink_clip_rect(rect);
        child.set_min_size(rect.size());
        child
    }
}

#[derive(Clone, Copy)]
struct SplitState {
    ratio: f32,
    drag_start: f32,
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    fn component(self, vector: Vec2) -> f32 {
        match self {
            Self::Horizontal => vector.x,
            Self::Vertical => vector.y,
        }
    }

    fn cursor(self) -> CursorIcon {
        match self {
            Self::Horizontal => CursorIcon::ResizeHorizontal,
            Self::Vertical => CursorIcon::ResizeVertical,
        }
    }

    fn rects(self, rect: Rect, first_size: f32, gap: f32) -> (Rect, Rect, Rect) {
        let mut first = rect;
        let mut divider = rect;
        let mut second = rect;
        match self {
            Self::Horizontal => {
                first.max.x = rect.min.x + first_size;
                divider.min.x = first.max.x;
                divider.max.x = divider.min.x + gap;
                second.min.x = divider.max.x;
            }
            Self::Vertical => {
                first.max.y = rect.min.y + first_size;
                divider.min.y = first.max.y;
                divider.max.y = divider.min.y + gap;
                second.min.y = divider.max.y;
            }
        }
        (first, divider, second)
    }
}
