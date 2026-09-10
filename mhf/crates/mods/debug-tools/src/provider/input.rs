//! Monster controls sampled independently of the debugger window's visibility.

use super::{DebugCommand, DebugControl, DebugSnapshot, MonsterAction, MonsterInput};
use egui::{Context, InputState, Key};

pub(crate) struct InputController {
    species: u8,
    variant: u8,
    pub(super) speed: f32,
    pub(super) shortcuts: [Option<MonsterAction>; 4],
    command_error: Option<String>,
}

impl Default for InputController {
    fn default() -> Self {
        Self {
            species: 94,
            variant: 0,
            speed: 300.0,
            shortcuts: [None; 4],
            command_error: None,
        }
    }
}

impl InputController {
    pub(super) fn species(&self) -> u8 {
        self.species
    }

    pub(super) fn variant(&self) -> u8 {
        self.variant
    }

    pub(super) fn select_species(&mut self, species: u8) {
        if self.species != species {
            self.species = species;
            self.variant = 0;
            self.shortcuts = [None; 4];
        }
    }

    pub(super) fn select_variant(&mut self, variant: u8) {
        if self.variant != variant {
            self.variant = variant;
            self.shortcuts = [None; 4];
        }
    }

    pub(super) fn take_command_error(&mut self) -> Option<String> {
        self.command_error.take()
    }

    /// Run after the window has updated settings and keyboard capture, including
    /// frames where that window is closed. Publishing neutral movement on focus
    /// loss complements the control channel's 200 ms expiry when frames stop.
    pub(crate) fn update(
        &mut self,
        context: &Context,
        control: &DebugControl,
        snapshot: &DebugSnapshot,
        window_capture: bool,
    ) {
        let capture = window_capture || context.egui_wants_keyboard_input();
        let sample = context.input(|input| self.sample(input, snapshot, capture));
        for command in sample.commands {
            self.command_error = Some(control.send(command).err().unwrap_or_default());
        }
        control.set_monster_input(sample.movement);
    }

    fn sample(&self, input: &InputState, snapshot: &DebugSnapshot, capture: bool) -> Sample {
        if !snapshot.controlling_monster || capture || !input.focused {
            return Sample::default();
        }
        let movement = MonsterInput {
            forward: f32::from(u8::from(input.key_down(Key::W)))
                - f32::from(u8::from(input.key_down(Key::S))),
            sideways: f32::from(u8::from(input.key_down(Key::D)))
                - f32::from(u8::from(input.key_down(Key::A))),
            vertical: f32::from(u8::from(input.key_down(Key::E)))
                - f32::from(u8::from(input.key_down(Key::Q))),
            speed: self.speed * if input.modifiers.shift { 3.0 } else { 1.0 },
        };
        let mut commands = Vec::new();
        if snapshot.monster == Some(self.species) && snapshot.monster_variant == self.variant {
            for (slot, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                .into_iter()
                .enumerate()
            {
                if input.key_pressed(key)
                    && let Some(action) = self.shortcuts[slot]
                {
                    commands.push(DebugCommand::MonsterAction(action));
                }
            }
        }
        if input.key_pressed(Key::R) {
            commands.push(DebugCommand::NextMonsterAction);
        }
        if input.key_pressed(Key::Backspace) {
            commands.push(DebugCommand::RestoreHunter);
        }
        Sample { movement, commands }
    }
}

#[derive(Default)]
struct Sample {
    movement: MonsterInput,
    commands: Vec<DebugCommand>,
}

#[cfg(test)]
mod tests;
