/// A client version encoded as three ASCII digits after a Sign command.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ClientVersion(u16);

impl ClientVersion {
    pub(crate) const ENCODED_LEN: usize = 3;

    const MAX: u16 = 999;

    pub(crate) const fn new(value: u16) -> Self {
        assert!(
            value <= Self::MAX,
            "client version must fit three decimal digits"
        );
        Self(value)
    }

    pub(crate) fn parse(encoded: &[u8]) -> Option<Self> {
        let encoded = std::str::from_utf8(encoded).ok()?;

        if encoded.len() != Self::ENCODED_LEN
            || !encoded.bytes().all(|digit| digit.is_ascii_digit())
        {
            return None;
        }

        encoded.parse::<u16>().ok().map(Self::new)
    }

    pub(crate) fn values() -> impl Iterator<Item = Self> {
        (0..=Self::MAX).map(Self::new)
    }

    pub(crate) const fn number(self) -> u16 {
        self.0
    }
}
