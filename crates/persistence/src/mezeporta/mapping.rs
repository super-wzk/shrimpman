use shrimpman_domain::mezeporta::MezeportaStall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, toasty::Embed)]
pub(crate) enum StoredMezeportaStall {
    TokotokoPartnya,
    Unknown3,
    VolpakkunTogether,
    Unknown5,
    Unknown6,
    Unknown7,
    Unknown8,
    Unknown9,
    Unknown10,
}

impl From<StoredMezeportaStall> for MezeportaStall {
    fn from(stall: StoredMezeportaStall) -> Self {
        match stall {
            StoredMezeportaStall::TokotokoPartnya => Self::TokotokoPartnya,
            StoredMezeportaStall::Unknown3 => Self::Unknown3,
            StoredMezeportaStall::VolpakkunTogether => Self::VolpakkunTogether,
            StoredMezeportaStall::Unknown5 => Self::Unknown5,
            StoredMezeportaStall::Unknown6 => Self::Unknown6,
            StoredMezeportaStall::Unknown7 => Self::Unknown7,
            StoredMezeportaStall::Unknown8 => Self::Unknown8,
            StoredMezeportaStall::Unknown9 => Self::Unknown9,
            StoredMezeportaStall::Unknown10 => Self::Unknown10,
        }
    }
}

impl From<MezeportaStall> for StoredMezeportaStall {
    fn from(stall: MezeportaStall) -> Self {
        match stall {
            MezeportaStall::TokotokoPartnya => Self::TokotokoPartnya,
            MezeportaStall::Unknown3 => Self::Unknown3,
            MezeportaStall::VolpakkunTogether => Self::VolpakkunTogether,
            MezeportaStall::Unknown5 => Self::Unknown5,
            MezeportaStall::Unknown6 => Self::Unknown6,
            MezeportaStall::Unknown7 => Self::Unknown7,
            MezeportaStall::Unknown8 => Self::Unknown8,
            MezeportaStall::Unknown9 => Self::Unknown9,
            MezeportaStall::Unknown10 => Self::Unknown10,
        }
    }
}
