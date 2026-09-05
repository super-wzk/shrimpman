use jiff::Timestamp;
use shrimpman_domain::{TimeRange, mezeporta::MezeportaFesta};
use toasty::Db;

use super::{MezeportaFestaRow, MezeportaFestaStallRow};

/// Toasty-backed Mezeporta Festa persistence.
#[derive(Debug, Clone)]
pub struct MezeportaFestaRepository {
    db: Db,
}

impl MezeportaFestaRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_active_at(
        &self,
        timestamp: Timestamp,
    ) -> toasty::Result<Option<MezeportaFesta>> {
        let mut db = self.db.clone();
        let Some(festa) = toasty::query!(
            MezeportaFestaRow FILTER
                .starts_at <= #timestamp
                AND .expires_at >= #timestamp
        )
        .order_by(MezeportaFestaRow::fields().starts_at().desc())
        .order_by(MezeportaFestaRow::fields().id().desc())
        .first()
        .exec(&mut db)
        .await?
        else {
            return Ok(None);
        };
        let period = TimeRange::new(festa.starts_at, festa.expires_at);
        let stalls = festa
            .stalls()
            .order_by(MezeportaFestaStallRow::fields().position().asc())
            .exec(&mut db)
            .await?;

        Ok(Some(MezeportaFesta {
            id: festa.id,
            period,
            solo_ticket_allowance: festa.solo_ticket_allowance,
            group_ticket_allowance: festa.group_ticket_allowance,
            stalls: stalls.into_iter().map(|stall| stall.stall.into()).collect(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;
    use shrimpman_domain::{TimeRange, mezeporta::MezeportaStall};

    use super::*;
    use crate::mezeporta::StoredMezeportaStall;

    #[tokio::test]
    async fn loads_the_active_festa_with_ordered_stalls() {
        let db = crate::test_database().await;
        let now = Timestamp::new(1_800_000_000, 0).unwrap();
        let period = TimeRange::from_duration(now, SignedDuration::from_hours(1));
        let mut connection = db.clone();
        let starts_at = period.starts_at();
        let expires_at = period.expires_at();
        let festa = toasty::create!(MezeportaFestaRow {
            starts_at,
            expires_at,
            solo_ticket_allowance: 5,
            group_ticket_allowance: 1,
        })
        .exec(&mut connection)
        .await
        .unwrap();

        for (position, stall) in [
            (1, StoredMezeportaStall::VolpakkunTogether),
            (0, StoredMezeportaStall::Unknown3),
        ] {
            toasty::create!(MezeportaFestaStallRow {
                festa_id: festa.id,
                position,
                stall,
            })
            .exec(&mut connection)
            .await
            .unwrap();
        }

        let loaded = MezeportaFestaRepository::new(&db)
            .find_active_at(now)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.id, festa.id);
        assert_eq!(loaded.period, period);
        assert_eq!(loaded.solo_ticket_allowance, 5);
        assert_eq!(loaded.group_ticket_allowance, 1);
        assert_eq!(
            loaded.stalls,
            [MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether,]
        );
    }
}
