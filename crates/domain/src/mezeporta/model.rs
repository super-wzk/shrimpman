use crate::TimeRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MezeportaFesta {
    pub id: u32,
    pub period: TimeRange,
    pub solo_ticket_allowance: u32,
    pub group_ticket_allowance: u32,
    pub stalls: Vec<MezeportaStall>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MezeportaStall {
    TokotokoPartnya = 2,
    Unknown3 = 3,
    VolpakkunTogether = 4,
    Unknown5 = 5,
    Unknown6 = 6,
    Unknown7 = 7,
    Unknown8 = 8,
    Unknown9 = 9,
    Unknown10 = 10,
}
