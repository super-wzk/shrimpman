use shrimpman_domain::account::{Account, AccountId, CourseRights};

use super::AccountRow;

impl From<AccountRow> for Account {
    fn from(account: AccountRow) -> Self {
        Self {
            id: AccountId::from(account.id),
            username: account.username,
            password_hash: account.password_hash,
            rights: CourseRights::from_bits_retain(account.rights),
        }
    }
}
