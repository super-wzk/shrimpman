use jiff::Timestamp;

use super::StoredMezeportaStall;

#[derive(Debug, toasty::Model)]
#[table = "mezeporta_festivals"]
pub(crate) struct MezeportaFestivalRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    #[column("period_starts_at")]
    pub(super) starts_at: Timestamp,

    #[column("period_expires_at")]
    pub(super) expires_at: Timestamp,

    pub(super) solo_ticket_allowance: u32,
    pub(super) group_ticket_allowance: u32,

    #[has_many(pair = festival)]
    stalls: toasty::Deferred<Vec<MezeportaFestivalStallRow>>,
}

#[derive(Debug, toasty::Model)]
#[table = "mezeporta_festival_stalls"]
#[key(festival_id, position)]
#[unique(festival_id, stall)]
pub(crate) struct MezeportaFestivalStallRow {
    pub(super) festival_id: u32,

    #[belongs_to]
    festival: toasty::Deferred<MezeportaFestivalRow>,

    pub(super) position: u8,
    pub(super) stall: StoredMezeportaStall,
}
