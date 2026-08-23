use shrimpman_domain::{
    account::Account,
    character::{Character, CharacterSignInHistory},
};
use toasty::Db;

use super::{CharacterRow, CharacterSignInRecordRow};

/// Toasty-backed character persistence.
#[derive(Debug, Clone)]
pub struct CharacterRepository {
    db: Db,
}

impl CharacterRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn list_active(&self, account: &Account) -> toasty::Result<Vec<Character>> {
        let mut db = self.db.clone();
        let account_id = u32::from(account.id);
        let characters = toasty::query!(CharacterRow FILTER .account_id == #account_id)
            .filter(CharacterRow::fields().deleted_at().is_none())
            .order_by(CharacterRow::fields().id().asc())
            .exec(&mut db)
            .await?;

        Ok(characters.into_iter().map(Character::from).collect())
    }

    pub async fn create_new(&self, account: &Account) -> toasty::Result<Character> {
        let mut db = self.db.clone();
        let account_id = u32::from(account.id);
        let character = toasty::create!(CharacterRow { account_id })
            .exec(&mut db)
            .await?;

        Ok(character.into())
    }

    pub async fn sign_in_history(
        &self,
        characters: &[Character],
    ) -> toasty::Result<CharacterSignInHistory> {
        if characters.is_empty() {
            return Ok(CharacterSignInHistory::default());
        }

        let queries = characters
            .iter()
            .map(|character| {
                CharacterRow::filter_by_id(u32::from(character.id))
                    .sign_in_records()
                    .order_by(CharacterSignInRecordRow::fields().signed_in_at().desc())
                    .order_by(CharacterSignInRecordRow::fields().id().desc())
                    .limit(1)
            })
            .collect::<Vec<_>>();
        let mut db = self.db.clone();
        let sign_ins = toasty::batch(queries).exec(&mut db).await?;

        let sign_ins = sign_ins
            .into_iter()
            .zip(characters)
            .filter_map(|(records, character)| {
                records
                    .into_iter()
                    .next()
                    .map(|record| (record.id, character.id, record.signed_in_at))
            })
            .collect::<Vec<_>>();
        let last_character_id = sign_ins
            .iter()
            .max_by_key(|(record_id, character_id, signed_in_at)| {
                (*signed_in_at, *record_id, *character_id)
            })
            .map(|(_, character_id, _)| *character_id);

        Ok(CharacterSignInHistory::new(
            last_character_id,
            sign_ins
                .into_iter()
                .map(|(_, character_id, signed_in_at)| (character_id, signed_in_at)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use jiff::{SignedDuration, Timestamp};

    use super::*;
    use crate::AccountRepository;

    #[tokio::test]
    async fn loads_latest_sign_ins_in_one_batch() {
        let db = crate::test_database().await;
        let account = AccountRepository::new(&db)
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let repository = CharacterRepository::new(&db);
        let first_character = repository.create_new(&account).await.unwrap();
        let second_character = repository.create_new(&account).await.unwrap();
        let first_character_id = first_character.id;
        let second_character_id = second_character.id;
        let first = Timestamp::new(1_700_000_000, 0).unwrap();
        let later = first + SignedDuration::from_hours(1);
        let latest = later + SignedDuration::from_hours(1);
        let mut db = db.clone();

        for (character_id, signed_in_at) in [
            (first_character_id, first),
            (first_character_id, latest),
            (second_character_id, later),
        ] {
            let character = CharacterRow::get_by_id(&mut db, u32::from(character_id))
                .await
                .unwrap();
            toasty::create!(in character.sign_in_records() {
                signed_in_at,
            })
            .exec(&mut db)
            .await
            .unwrap();
        }

        let history = repository
            .sign_in_history(&[first_character, second_character])
            .await
            .unwrap();

        assert_eq!(history.last_sign_in_at(first_character_id), Some(latest));
        assert_eq!(history.last_sign_in_at(second_character_id), Some(later));
        assert_eq!(history.last_character_id(), Some(first_character_id));
    }
}
