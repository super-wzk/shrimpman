use jiff::{SignedDuration, Timestamp};

/// A bounded range on the timestamp timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    starts_at: Timestamp,
    expires_at: Timestamp,
}

impl TimeRange {
    pub const fn new(starts_at: Timestamp, expires_at: Timestamp) -> Self {
        Self {
            starts_at,
            expires_at,
        }
    }

    pub fn from_duration(starts_at: Timestamp, duration: SignedDuration) -> Self {
        Self::new(starts_at, starts_at + duration)
    }

    pub const fn starts_at(self) -> Timestamp {
        self.starts_at
    }

    pub const fn expires_at(self) -> Timestamp {
        self.expires_at
    }

    pub fn contains(self, timestamp: Timestamp) -> bool {
        self.starts_at <= timestamp && timestamp <= self.expires_at
    }

    pub fn is_expired_at(self, timestamp: Timestamp) -> bool {
        self.expires_at < timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_range_from_a_duration() {
        let starts_at = Timestamp::new(1_700_000_000, 0).unwrap();
        let duration = SignedDuration::from_hours(24);
        let range = TimeRange::from_duration(starts_at, duration);

        assert_eq!(range.starts_at(), starts_at);
        assert_eq!(range.expires_at(), starts_at + duration);
    }

    #[test]
    fn expires_after_its_expiration_timestamp() {
        let starts_at = Timestamp::new(1_700_000_000, 0).unwrap();
        let duration = SignedDuration::from_hours(24);
        let range = TimeRange::from_duration(starts_at, duration);

        assert!(!range.is_expired_at(range.expires_at()));
        assert!(range.is_expired_at(range.expires_at() + SignedDuration::from_nanos(1)));
    }

    #[test]
    fn contains_both_boundaries() {
        let starts_at = Timestamp::new(1_700_000_000, 0).unwrap();
        let range = TimeRange::from_duration(starts_at, SignedDuration::from_hours(24));

        assert!(range.contains(range.starts_at()));
        assert!(range.contains(range.expires_at()));
        assert!(!range.contains(range.starts_at() - SignedDuration::from_nanos(1)));
        assert!(!range.contains(range.expires_at() + SignedDuration::from_nanos(1)));
    }
}
