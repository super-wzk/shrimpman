use jiff::Timestamp;
use shrimpman_domain::{TimeRange, mezeporta::MezeportaFestival};
use toasty::Db;

use super::{MezeportaFestivalRow, MezeportaFestivalStallRow};

/// Toasty-backed Mezeporta Festival persistence.
#[derive(Debug, Clone)]
pub struct MezeportaFestivalRepository {
    db: Db,
}

impl MezeportaFestivalRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_active_at(
        &self,
        timestamp: Timestamp,
    ) -> toasty::Result<Option<MezeportaFestival>> {
        let mut db = self.db.clone();
        let Some(festival) = toasty::query!(
            MezeportaFestivalRow FILTER
                .starts_at <= #timestamp
                AND .expires_at >= #timestamp
        )
        .order_by(MezeportaFestivalRow::fields().starts_at().desc())
        .order_by(MezeportaFestivalRow::fields().id().desc())
        .first()
        .exec(&mut db)
        .await?
        else {
            return Ok(None);
        };
        let period = TimeRange::new(festival.starts_at, festival.expires_at);
        let stalls = festival
            .stalls()
            .order_by(MezeportaFestivalStallRow::fields().position().asc())
            .exec(&mut db)
            .await?;

        Ok(Some(MezeportaFestival {
            id: festival.id,
            period,
            solo_ticket_allowance: festival.solo_ticket_allowance,
            group_ticket_allowance: festival.group_ticket_allowance,
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
    async fn loads_the_active_festival_with_ordered_stalls() {
        let db = crate::test_database().await;
        let now = Timestamp::new(1_800_000_000, 0).unwrap();
        let period = TimeRange::from_duration(now, SignedDuration::from_hours(1));
        let mut connection = db.clone();
        let starts_at = period.starts_at();
        let expires_at = period.expires_at();
        let festival = toasty::create!(MezeportaFestivalRow {
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
            toasty::create!(MezeportaFestivalStallRow {
                festival_id: festival.id,
                position,
                stall,
            })
            .exec(&mut connection)
            .await
            .unwrap();
        }

        let loaded = MezeportaFestivalRepository::new(&db)
            .find_active_at(now)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.id, festival.id);
        assert_eq!(loaded.period, period);
        assert_eq!(loaded.solo_ticket_allowance, 5);
        assert_eq!(loaded.group_ticket_allowance, 1);
        assert_eq!(
            loaded.stalls,
            [MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether,]
        );
    }
}
