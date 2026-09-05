use derive_more::{From, Into};

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, From, Into)]
pub struct AccountId(u32);

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CourseRights: u32 {
        const TRIAL = 1 << 1;
        const HUNTER_LIFE = 1 << 2;
        const EXTRA_A = 1 << 3;
        const EXTRA_B = 1 << 4;
        const MOBILE = 1 << 5;
        const PREMIUM = 1 << 6;
        const PALLONE = 1 << 7;
        const ASSIST = 1 << 8;
        const N_COURSE = 1 << 9;
        const HIDEN = 1 << 10;
        const HUNTER_SUPPORT = 1 << 11;
        const N_BOOST = 1 << 12;
        const DEBUG = 1 << 20;
        const COG_LINK_EXPIRED = 1 << 21;
        const XBOX_360_GOLD = 1 << 22;
        const PS3_TROPHY = 1 << 23;
        const COG = 1 << 24;
        const CAFE_SP = 1 << 25;
        const NET_CAFE = 1 << 26;
        const HUNTER_LIFE_RENEWING = 1 << 27;
        const EXTRA_RENEWING = 1 << 28;
        const FREE = 1 << 29;
        const REAL_NET_CAFE = 1 << 30;
        const HIDEN_EXTRA = 1 << 31;
    }
}

/// An authenticated account and its service entitlements.
pub struct Account {
    pub id: AccountId,
    pub username: String,
    pub password_hash: String,
    pub rights: CourseRights,
}
