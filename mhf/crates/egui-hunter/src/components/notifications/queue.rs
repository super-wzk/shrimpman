use std::{collections::VecDeque, time::Duration};

use egui::{Context, Id};

use crate::NoticeKind;

#[derive(Debug)]
pub struct Notification {
    pub id: Id,
    pub kind: NoticeKind,
    pub text: String,
    duration: Duration,
    shown_at: Option<f64>,
}

/// A bounded FIFO of compact toasts, with one visible at a time.
/// A queued item's lifetime starts when it is first displayed. Uses egui
/// time/repaint, with no background timer. Use a dialog for actions needing input.
#[derive(Debug)]
pub struct Notifications {
    pub(super) id: Id,
    next: u64,
    capacity: usize,
    pub(super) pass_through: bool,
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
            pass_through: true,
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

    /// Whether pointer events reach controls behind the toast. Defaults to true.
    /// This only controls egui hit testing; the host decides game input capture.
    pub fn set_pass_through(&mut self, pass_through: bool) {
        self.pass_through = pass_through;
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

    /// Advance expiration and start the next item's lifetime. Call this only
    /// when presenting the queue; rendering and positioning are caller-owned.
    pub fn advance(&mut self, ctx: &Context) -> Option<&Notification> {
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
        Some(notice)
    }
}
