//! Account return-period rules.

use jiff::{SignedDuration, Timestamp};

use crate::TimeRange;

const RETURN_PERIOD: SignedDuration = SignedDuration::from_hours(30 * 24);
const RETURN_THRESHOLD: SignedDuration = SignedDuration::from_hours(90 * 24);

pub fn starting_at(starts_at: Timestamp) -> TimeRange {
    TimeRange::from_duration(starts_at, RETURN_PERIOD)
}

pub fn should_start(last_sign_in_at: Option<Timestamp>, signed_in_at: Timestamp) -> bool {
    last_sign_in_at.is_none_or(|last_sign_in_at| {
        TimeRange::from_duration(last_sign_in_at, RETURN_THRESHOLD).is_expired_at(signed_in_at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_a_return_period_for_the_first_sign_in() {
        let signed_in_at = Timestamp::new(1_700_000_000, 0).unwrap();

        assert!(should_start(None, signed_in_at));
    }

    #[test]
    fn starts_a_new_return_period_after_ninety_days() {
        let first = Timestamp::new(1_700_000_000, 0).unwrap();
        let returning = first + RETURN_THRESHOLD + SignedDuration::from_hours(1);

        assert!(should_start(Some(first), returning));
    }

    #[test]
    fn keeps_the_existing_return_period_for_regular_sign_ins() {
        let first = Timestamp::new(1_700_000_000, 0).unwrap();
        let later = first + SignedDuration::from_hours(1);

        assert!(!should_start(Some(first), later));
    }

    #[test]
    fn return_period_lasts_thirty_days() {
        let starts_at = Timestamp::new(1_700_000_000, 0).unwrap();
        let period = starting_at(starts_at);

        assert_eq!(period.starts_at(), starts_at);
        assert_eq!(period.expires_at(), starts_at + RETURN_PERIOD);
    }
}
