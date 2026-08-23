use jiff::Timestamp;
use shrimpman_domain::TimeRange;

#[derive(Debug, Clone, Copy, toasty::Embed)]
pub(crate) struct StoredTimeRange {
    starts_at: Timestamp,
    expires_at: Timestamp,
}

impl From<TimeRange> for StoredTimeRange {
    fn from(period: TimeRange) -> Self {
        Self {
            starts_at: period.starts_at(),
            expires_at: period.expires_at(),
        }
    }
}

impl From<StoredTimeRange> for TimeRange {
    fn from(period: StoredTimeRange) -> Self {
        Self::new(period.starts_at, period.expires_at)
    }
}
