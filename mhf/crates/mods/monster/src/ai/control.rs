//! The event-slot mapping the product path names.
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
/// design: the client evidence establishes the bit and slot, but does not
/// establish a user-facing event name such as "flash" or "roar".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventSlot {
    pub mask: u8,
    pub root_index: usize,
}

/// Event-slot mapping recovered from the native selector.
///
/// Keep this order stable.  It is also the native priority order, which is the
/// order the client tests the bits in.
pub const EVENT_SLOTS: [EventSlot; 7] = [
    EventSlot {
        mask: 0x40,
        root_index: 14,
    },
    EventSlot {
        mask: 0x80,
        root_index: 13,
    },
    EventSlot {
        mask: 0x20,
        root_index: 4,
    },
    EventSlot {
        mask: 0x10,
        root_index: 3,
    },
    EventSlot {
        mask: 0x08,
        root_index: 11,
    },
    EventSlot {
        mask: 0x04,
        root_index: 10,
    },
    EventSlot {
        mask: 0x02,
        root_index: 8,
    },
];

/// The one event slot that shares its table with an `act`-indexed array.
///
/// `route_ptr_set` (`0x108604C0`) returns `root[8][act]`, where `act` is the
/// byte `em->cmd_route_act` (`+2595`): the same table whose `cell[0]` the
/// selector runs for mask `0x02` also holds one route script per act. A binding
/// therefore keeps the whole byte-indexed window for this slot instead of the
/// declaration's own extent.
pub const ROUTE_ROOT_INDEX: usize = 8;
