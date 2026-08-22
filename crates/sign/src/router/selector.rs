use std::ops::Range;

use shrimpman_protocol::RouteSelector;

use crate::ClientVersion;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "version-specific packet registrations have not been added yet"
    )
)]
#[derive(Clone)]
pub(crate) enum VersionSelector {
    Any,
    Exact(ClientVersion),
    Before(ClientVersion),
    From(ClientVersion),
    Range(Range<ClientVersion>),
}

impl RouteSelector for VersionSelector {
    type Metadata = ClientVersion;
    type Priority = u8;
    type Conflict = ClientVersion;

    fn matches(&self, version: &ClientVersion) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => version == expected,
            Self::Before(end) => version < end,
            Self::From(start) => version >= start,
            Self::Range(range) => range.contains(version),
        }
    }

    fn priority(&self) -> Self::Priority {
        match self {
            Self::Any => 0,
            Self::Exact(_) => 2,
            Self::Before(_) | Self::From(_) | Self::Range(_) => 1,
        }
    }

    fn is_empty(&self) -> bool {
        !(0..=999)
            .map(ClientVersion::new)
            .any(|version| self.matches(&version))
    }

    fn conflict(&self, other: &Self) -> Option<Self::Conflict> {
        (0..=999)
            .map(ClientVersion::new)
            .find(|version| self.matches(version) && other.matches(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V041: ClientVersion = ClientVersion::new(41);
    const V100: ClientVersion = ClientVersion::new(100);

    #[test]
    fn matches_half_open_and_one_sided_ranges() {
        assert!(VersionSelector::Before(V100).matches(&V041));
        assert!(!VersionSelector::Before(V100).matches(&V100));
        assert!(VersionSelector::From(V100).matches(&V100));
        assert!(VersionSelector::Range(V041..V100).matches(&V041));
        assert!(!VersionSelector::Range(V041..V100).matches(&V100));
    }

    #[test]
    fn detects_overlapping_ranges() {
        assert_eq!(
            VersionSelector::From(V041)
                .conflict(&VersionSelector::Before(V100))
                .map(ClientVersion::number),
            Some(41)
        );
    }
}
