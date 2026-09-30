//! DAT[389] weapon actions read by native `10A67790`, `10A68110` and `10A68510`.
//! Record offsets are relative to the supplied DAT bytes, including when its
//! pointers have been relocated. Weapon callbacks retain their own opcode domain.

use std::ops::Range;

use crate::{Error, PathSegment, ResourcePath, Result};

mod attack_directory;
pub use attack_directory::AttackDirectory;

pub const DAT_ROOT: u32 = 389;
pub const WEAPON_COUNT: u8 = 14;
pub const ACTION_SIZE: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponActions {
    /// Eight-byte weapon-directory record in the supplied DAT image.
    pub offset: usize,
    pub count: u32,
    pub records: Range<usize>,
}

/// Read one weapon's directory without requiring other weapons' action tables
/// to be valid. `base` is zero for file offsets, or the loaded DAT image's base.
pub fn weapon_actions(bytes: &[u8], base: u32, weapon: u8) -> Result<WeaponActions> {
    if weapon >= WEAPON_COUNT {
        return Err(Error::new(
            DAT_ROOT as usize * 4,
            "unsupported weapon action directory",
        ));
    }
    let field = DAT_ROOT as usize * 4;
    let directory = table(
        bytes,
        base,
        dword(bytes, field)?,
        u32::from(WEAPON_COUNT),
        8,
        field,
    )?;
    let offset = directory.start + usize::from(weapon) * 8;
    let count = dword(bytes, offset)?;
    if count > 256 {
        return Err(Error::new(offset, "weapon action count exceeds 256"));
    }
    let records = table(
        bytes,
        base,
        dword(bytes, offset + 4)?,
        count,
        ACTION_SIZE,
        offset + 4,
    )?;
    Ok(WeaponActions {
        offset,
        count,
        records,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub weapon: u8,
    pub action: u16,
    /// Twenty-four-byte action-directory record in the supplied DAT image.
    pub offset: usize,
    pub steps_range: Range<usize>,
    pub transitions_range: Range<usize>,
    pub events_range: Range<usize>,
    pub steps: Vec<ActionStep>,
    pub transitions: Vec<ActionTransition>,
    pub events: Vec<ActionEvent>,
}

impl Definition {
    pub fn parse(bytes: &[u8], base: u32, weapon: u8, action: u16) -> Result<Self> {
        let directory = weapon_actions(bytes, base, weapon)?;
        if u32::from(action) >= directory.count {
            return Err(Error::new(
                directory.offset,
                "weapon action index exceeds its directory",
            ));
        }
        let offset = directory.records.start + usize::from(action) * ACTION_SIZE;
        let steps_range = table(
            bytes,
            base,
            dword(bytes, offset + 4)?,
            dword(bytes, offset)?,
            ActionStep::SIZE,
            offset + 4,
        )?;
        let transitions_range = table(
            bytes,
            base,
            dword(bytes, offset + 12)?,
            dword(bytes, offset + 8)?,
            ActionTransition::SIZE,
            offset + 12,
        )?;
        let events_range = table(
            bytes,
            base,
            dword(bytes, offset + 20)?,
            dword(bytes, offset + 16)?,
            ActionEvent::SIZE,
            offset + 20,
        )?;
        let steps = bytes[steps_range.clone()]
            .as_chunks::<{ ActionStep::SIZE }>()
            .0
            .iter()
            .map(|record| {
                ActionStep(std::array::from_fn(|index| {
                    u16::from_le_bytes([record[index * 2], record[index * 2 + 1]])
                }))
            })
            .collect();
        let transitions = bytes[transitions_range.clone()]
            .as_chunks::<{ ActionTransition::SIZE }>()
            .0
            .iter()
            .map(|record| ActionTransition::parse(record))
            .collect::<Result<_>>()?;
        let events = bytes[events_range.clone()]
            .as_chunks::<{ ActionEvent::SIZE }>()
            .0
            .iter()
            .map(|record| ActionEvent {
                step: u16::from_le_bytes([record[0], record[1]]),
                timing: record[2],
                phase: record[3] as i8,
                frame: u16::from_le_bytes([record[4], record[5]]),
                count: u16::from_le_bytes([record[6], record[7]]),
                operation: u16::from_le_bytes([record[8], record[9]]),
                argument: u16::from_le_bytes([record[10], record[11]]),
            })
            .collect();
        Ok(Self {
            weapon,
            action,
            offset,
            steps_range,
            transitions_range,
            events_range,
            steps,
            transitions,
            events,
        })
    }

    pub fn step_span(&self, index: usize) -> Option<Range<usize>> {
        record_span(&self.steps_range, index, self.steps.len(), ActionStep::SIZE)
    }

    pub fn event_span(&self, index: usize) -> Option<Range<usize>> {
        record_span(
            &self.events_range,
            index,
            self.events.len(),
            ActionEvent::SIZE,
        )
    }

    pub fn transition_span(&self, index: usize) -> Option<Range<usize>> {
        record_span(
            &self.transitions_range,
            index,
            self.transitions.len(),
            ActionTransition::SIZE,
        )
    }

    pub fn resource_path(&self, source: &str) -> Result<ResourcePath> {
        ResourcePath::from_parts(
            source,
            [
                PathSegment::Index(DAT_ROOT),
                PathSegment::Index(u32::from(self.weapon)),
                PathSegment::Index(u32::from(self.action)),
            ],
        )
        .map_err(|error| Error::new(self.offset, error.to_string()))
    }

    pub fn step_path(&self, source: &str, index: usize) -> Option<ResourcePath> {
        self.step_span(index)?;
        self.record_path(source, "steps", index)
    }

    pub fn event_path(&self, source: &str, index: usize) -> Option<ResourcePath> {
        self.event_span(index)?;
        self.record_path(source, "events", index)
    }

    pub fn transition_path(&self, source: &str, index: usize) -> Option<ResourcePath> {
        self.transition_span(index)?;
        self.record_path(source, "transitions", index)
    }

    fn record_path(&self, source: &str, table: &str, index: usize) -> Option<ResourcePath> {
        let mut path = self.resource_path(source).ok()?;
        path.push(PathSegment::Field(table.into())).ok()?;
        path.push(PathSegment::Index(u32::try_from(index).ok()?))
            .ok()?;
        Some(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionStep(pub [u16; 6]);

impl ActionStep {
    pub const SIZE: usize = 12;

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (index, word) in self.0.into_iter().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// Eight-byte native condition shared by transition windows and event timing.
/// Unknown timing values and signed phase bytes retain their stored values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionCondition {
    pub step: u16,
    pub timing: u8,
    pub phase: i8,
    pub frame: u16,
    pub count: u16,
}

impl ActionCondition {
    pub const SIZE: usize = 8;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; Self::SIZE] = bytes
            .try_into()
            .map_err(|_| Error::new(0, "expected 8 action condition bytes"))?;
        Ok(Self {
            step: u16::from_le_bytes([b[0], b[1]]),
            timing: b[2],
            phase: b[3] as i8,
            frame: u16::from_le_bytes([b[4], b[5]]),
            count: u16::from_le_bytes([b[6], b[7]]),
        })
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..2].copy_from_slice(&self.step.to_le_bytes());
        bytes[2] = self.timing;
        bytes[3] = self.phase as u8;
        bytes[4..6].copy_from_slice(&self.frame.to_le_bytes());
        bytes[6..].copy_from_slice(&self.count.to_le_bytes());
        bytes
    }
}

/// Forty-byte input/transition branch consumed by native `10A68110`.
/// Input, selection and argument retain raw values without inferred key names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionTransition {
    pub priority: u16,
    pub input: u16,
    pub selection: u16,
    pub input_start: ActionCondition,
    pub input_end: ActionCondition,
    pub argument: u16,
    pub transition_start: ActionCondition,
    pub transition_end: ActionCondition,
}

impl ActionTransition {
    pub const SIZE: usize = 40;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; Self::SIZE] = bytes
            .try_into()
            .map_err(|_| Error::new(0, "expected 40 action transition bytes"))?;
        let word = |at| u16::from_le_bytes([b[at], b[at + 1]]);
        Ok(Self {
            priority: word(0),
            input: word(2),
            selection: word(4),
            input_start: ActionCondition::parse(&b[6..14])?,
            input_end: ActionCondition::parse(&b[14..22])?,
            argument: word(22),
            transition_start: ActionCondition::parse(&b[24..32])?,
            transition_end: ActionCondition::parse(&b[32..40])?,
        })
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (at, value) in [
            (0, self.priority),
            (2, self.input),
            (4, self.selection),
            (22, self.argument),
        ] {
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (at, condition) in [
            (6, self.input_start),
            (14, self.input_end),
            (24, self.transition_start),
            (32, self.transition_end),
        ] {
            bytes[at..at + ActionCondition::SIZE].copy_from_slice(&condition.to_bytes());
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionEvent {
    pub step: u16,
    pub timing: u8,
    pub phase: i8,
    pub frame: u16,
    pub count: u16,
    pub operation: u16,
    pub argument: u16,
}

impl ActionEvent {
    pub const SIZE: usize = 12;

    /// Only established weapon callback operations produce an attack reference.
    /// The raw operation and argument remain available, including unknown codes.
    pub fn attack_reference(&self, weapon: u8) -> Option<AttackReference> {
        let category = match (weapon, self.operation) {
            (0, 16 | 17)
            | (1 | 5, 8)
            | (2, 10)
            | (3 | 4, 12)
            | (6, 11)
            | (7, 17)
            | (8, 9)
            | (9, 10 | 11)
            | (10, 9) => 0,
            (11, 4 | 9 | 10) => 100,
            (12, 4) => 106,
            (12, 5) => 107,
            (13, 3) => 120,
            _ => return None,
        };
        Some(AttackReference {
            category,
            subtype: None,
            record: self.argument,
        })
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..2].copy_from_slice(&self.step.to_le_bytes());
        bytes[2] = self.timing;
        bytes[3] = self.phase as u8;
        for (index, value) in [self.frame, self.count, self.operation, self.argument]
            .into_iter()
            .enumerate()
        {
            bytes[4 + index * 2..6 + index * 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// SDT lookup keys, not directory indices. Ordinary attack lookup matches kind;
/// callers using the category-6 subtype lookup can retain that second key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackReference {
    pub category: u16,
    pub subtype: Option<u16>,
    pub record: u16,
}

/// The native bank, weapon and style remain available even when no resource
/// filename has been established for this motion selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeMotionRef {
    pub id: u16,
    pub weapon: u8,
    pub style: Option<u8>,
}

impl NativeMotionRef {
    pub fn bank(self) -> u16 {
        self.id / 1000
    }
    pub fn record(self) -> u32 {
        u32::from(self.id / 100 % 10)
    }
    pub fn slot(self) -> u32 {
        u32::from(self.id % 100)
    }

    pub fn resource_path(self) -> Option<ResourcePath> {
        let source = match self.bank() {
            1 if self.weapon == 11 => match self.style {
                Some(0) => "motion/w11.mot".into(),
                Some(1) => "motion/w11ten.mot".into(),
                Some(2) => "motion/w11ran.mot".into(),
                Some(3) => "motion/w11goku.mot".into(),
                _ => return None,
            },
            1 if self.weapon < WEAPON_COUNT => format!("motion/w{:02}.mot", self.weapon),
            3 => "motion/plface_m-pc.mot".into(),
            4 => "motion/plface_f-pc.mot".into(),
            _ => return None,
        };
        ResourcePath::from_parts(
            source,
            [
                PathSegment::Index(self.record()),
                PathSegment::Index(self.slot()),
            ],
        )
        .ok()
    }
}

fn record_span(
    table: &Range<usize>,
    index: usize,
    count: usize,
    stride: usize,
) -> Option<Range<usize>> {
    if index >= count {
        return None;
    }
    let start = table.start.checked_add(index.checked_mul(stride)?)?;
    let end = start.checked_add(stride)?;
    (end <= table.end).then_some(start..end)
}

fn dword(bytes: &[u8], offset: usize) -> Result<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .ok_or_else(|| Error::new(offset, "truncated weapon action field"))
}

fn table(
    bytes: &[u8],
    base: u32,
    pointer: u32,
    count: u32,
    stride: usize,
    field: usize,
) -> Result<Range<usize>> {
    if count > 4096 {
        return Err(Error::new(field, "weapon action record count exceeds 4096"));
    }
    if count == 0 {
        return Ok(0..0);
    }
    if pointer == 0 {
        return Err(Error::new(field, "null weapon action table pointer"));
    }
    let start = pointer
        .checked_sub(base)
        .ok_or_else(|| Error::new(field, "weapon action pointer precedes its DAT image"))?
        as usize;
    if start < crate::dat::HEADER_SIZE {
        return Err(Error::new(
            field,
            "weapon action pointer targets its DAT header",
        ));
    }
    let end = (count as usize)
        .checked_mul(stride)
        .and_then(|size| start.checked_add(size))
        .ok_or_else(|| Error::new(field, "weapon action table size overflow"))?;
    bytes
        .get(start..end)
        .ok_or_else(|| Error::new(field, "weapon action table exceeds its DAT image"))?;
    Ok(start..end)
}

#[cfg(test)]
mod tests;
