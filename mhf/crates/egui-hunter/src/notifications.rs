use std::{collections::VecDeque, time::Duration};

use egui::{Context, Id, InnerResponse, UiBuilder};

use crate::{NoticeKind, Theme};

#[derive(Debug)]
struct Notification {
    id: Id,
    kind: NoticeKind,
    text: String,
    duration: Duration,
    shown_at: Option<f64>,
}

/// A bounded FIFO with one visible notification. A queued item's lifetime starts
/// when it is first displayed. Uses egui time/repaint, with no background timer.
#[derive(Debug)]
pub struct Notifications {
    id: Id,
    next: u64,
    capacity: usize,
    queue: VecDeque<Notification>,
}

impl Notifications {
    pub fn new(id: Id) -> Self {
        Self::with_capacity(id, 32)
    }

    /// At capacity, discard the oldest waiting item, preserving the visible one
    /// when capacity > 1. Zero capacity is treated as one.
    pub fn with_capacity(id: Id, capacity: usize) -> Self {
        Self {
            id,
            next: 0,
            capacity: capacity.max(1),
            queue: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn front_id(&self) -> Option<Id> {
        self.queue.front().map(|notice| notice.id)
    }

    pub fn push(&mut self, ctx: &Context, kind: NoticeKind, text: impl Into<String>) -> Id {
        self.push_for(ctx, kind, text, Duration::from_secs(3))
    }

    pub fn push_for(
        &mut self,
        ctx: &Context,
        kind: NoticeKind,
        text: impl Into<String>,
        duration: Duration,
    ) -> Id {
        let id = self.id.with(self.next);
        self.next = self.next.wrapping_add(1);
        if self.queue.len() == self.capacity {
            let oldest_waiting = if self.capacity > 1 { 1 } else { 0 };
            self.queue.remove(oldest_waiting);
        }
        self.queue.push_back(Notification {
            id,
            kind,
            text: text.into(),
            duration,
            shown_at: None,
        });
        ctx.request_repaint();
        id
    }

    pub fn dismiss(&mut self, ctx: &Context, id: Id) -> bool {
        let Some(index) = self.queue.iter().position(|notice| notice.id == id) else {
            return false;
        };
        self.queue.remove(index);
        ctx.request_repaint();
        true
    }

    pub fn clear(&mut self, ctx: &Context) {
        self.queue.clear();
        ctx.request_repaint();
    }

    /// Render after your main UI. Each queue needs a unique ID. Keep calling
    /// while nonempty so expiry and pending notifications continue advancing.
    pub fn show(&mut self, ctx: &Context, theme: &Theme) -> Option<InnerResponse<()>> {
        let now = ctx.input(|i| i.time);
        let until = loop {
            let notice = self.queue.front_mut()?;
            let since = *notice.shown_at.get_or_insert(now);
            let duration = notice.duration.as_secs_f64();
            if now - since < duration {
                break since + duration;
            }
            self.queue.pop_front();
        };
        let notice = self.queue.front()?;
        ctx.request_repaint_after(Duration::from_secs_f64((until - now).max(0.0)));
        let mut dismissed = false;
        // Reuse a single Area ID so frequent messages do not accumulate areas.
        let output = egui::Area::new(self.id)
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(120.0, 520.0));
                ui.scope_builder(UiBuilder::new().id(notice.id), |ui| {
                    theme.panel("").show(ui, |ui| {
                        theme.notice(ui, notice.kind, &notice.text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            dismissed = ui
                                .add(theme.button("关闭").min_size(egui::vec2(56.0, 28.0)))
                                .clicked();
                        });
                    });
                });
            });
        if dismissed {
            self.dismiss(ctx, notice.id);
        }
        Some(output)
    }
}
