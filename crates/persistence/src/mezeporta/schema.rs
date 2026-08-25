use jiff::Timestamp;

use super::StoredMezeportaStall;

#[derive(Debug, toasty::Model)]
#[table = "mezeporta_festas"]
pub(crate) struct MezeportaFestaRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    #[column("period_starts_at")]
    pub(super) starts_at: Timestamp,

    #[column("period_expires_at")]
    pub(super) expires_at: Timestamp,

    pub(super) solo_ticket_allowance: u32,
    pub(super) group_ticket_allowance: u32,

    #[has_many(pair = festa)]
    stalls: toasty::Deferred<Vec<MezeportaFestaStallRow>>,
}

#[derive(Debug, toasty::Model)]
#[table = "mezeporta_festa_stalls"]
#[key(festa_id, position)]
#[unique(festa_id, stall)]
pub(crate) struct MezeportaFestaStallRow {
    pub(super) festa_id: u32,

    #[belongs_to]
    festa: toasty::Deferred<MezeportaFestaRow>,

    pub(super) position: u8,
    pub(super) stall: StoredMezeportaStall,
}
