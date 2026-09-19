//! Named DSL events and their native dispatch mapping.
//!
//! The native selector tests seven event bits in a fixed order and reaches
//! each one through its own root-table slot.  These constants are what the
//! compiler, the binding and the overlay share.

/// The root-table slot containing the main-script table.
pub const MAIN_ROOT_INDEX: usize = 0;

/// A native event bit and the root-table slot selected for that event.
///
/// A slot's position in [`EVENT_SLOTS`] is the order the native selector tests
/// the bits when more than one is present.  The mapping is mechanism-level by
/// design; the name describes the trigger, not the species-specific response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventSlot {
    pub name: &'static str,
    pub ending: u8,
    pub mask: u8,
    pub root_index: usize,
}

/// Event-slot mapping recovered from the native selector.
///
/// Keep this order stable.  It is also the native priority order, which is the
/// order the client tests the bits in.
pub const EVENT_SLOTS: [EventSlot; 7] = [
    EventSlot {
        name: "dung_reaction",
        ending: 0xf5,
        mask: 0x40,
        root_index: 14,
    },
    EventSlot {
        name: "invalid_ground",
        ending: 0xf6,
        mask: 0x80,
        root_index: 13,
    },
    EventSlot {
        name: "player_detected",
        ending: 0xfc,
        mask: 0x20,
        root_index: 4,
    },
    EventSlot {
        name: "awareness",
        ending: 0xfd,
        mask: 0x10,
        root_index: 3,
    },
    EventSlot {
        name: "rage_entered",
        ending: 0xf8,
        mask: 0x08,
        root_index: 11,
    },
    EventSlot {
        name: "group_signal",
        ending: 0xf9,
        mask: 0x04,
        root_index: 10,
    },
    EventSlot {
        name: "bait_detected",
        ending: 0xfa,
        mask: 0x02,
        root_index: 8,
    },
];

/// 108604C0 reads descriptor+8 (word 2), not descriptor word 8.
pub const ROUTE_ROOT_INDEX: usize = 2;
