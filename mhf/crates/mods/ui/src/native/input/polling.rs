/// Ownership is tracked from physical samples, separately from window messages:
/// DirectInput can be polled before or after the matching window event.
pub(super) struct PolledInput<const N: usize> {
    device: usize,
    owners: [Option<bool>; N],
}

impl<const N: usize> Default for PolledInput<N> {
    fn default() -> Self {
        Self {
            device: 0,
            owners: [None; N],
        }
    }
}

impl<const N: usize> PolledInput<N> {
    fn filter_buttons(
        &mut self,
        device: usize,
        buttons: &mut [u8],
        capture: bool,
        mut event_owner: impl FnMut(usize) -> Option<bool>,
    ) -> bool {
        if self.device != device {
            self.device = device;
            self.owners.fill(None);
        }
        self.owners[buttons.len()..].fill(None);
        for (index, (owner, button)) in self.owners.iter_mut().zip(buttons).enumerate() {
            if *button & 0x80 == 0 {
                *owner = None;
            } else if *owner.get_or_insert_with(|| event_owner(index).unwrap_or(capture)) {
                *button = 0;
            }
        }
        // A drag keeps the recipient of its held buttons, even across policy
        // changes. Otherwise movement and wheel input follow the current policy.
        if self.owners.iter().any(Option::is_some) {
            self.owners.contains(&Some(true))
        } else {
            capture
        }
    }
}

impl PolledInput<8> {
    pub(super) fn mouse(
        &mut self,
        device: usize,
        data: &mut [u8],
        capture: bool,
        event_owner: impl FnMut(usize) -> Option<bool>,
    ) {
        if !matches!(data.len(), 16 | 20) {
            return;
        }
        let (motion, buttons) = data.split_at_mut(12);
        if self.filter_buttons(device, buttons, capture, event_owner) {
            motion.fill(0);
        }
    }
}

impl PolledInput<256> {
    pub(super) fn keyboard(
        &mut self,
        device: usize,
        data: &mut [u8],
        capture: bool,
        event_owner: impl FnMut(usize) -> Option<bool>,
    ) {
        if data.len() == 256 {
            self.filter_buttons(device, data, capture, event_owner);
        }
    }
}

#[cfg(test)]
mod tests;
