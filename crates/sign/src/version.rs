/// A client version encoded as three ASCII digits after a Sign command.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientVersion(u16);

impl ClientVersion {
    pub const fn new(value: u16) -> Self {
        assert!(value <= 999, "client version must fit three decimal digits");
        Self(value)
    }

    pub const fn number(self) -> u16 {
        self.0
    }

    pub const fn digits(self) -> [u8; 3] {
        [
            b'0' + (self.0 / 100) as u8,
            b'0' + ((self.0 / 10) % 10) as u8,
            b'0' + (self.0 % 10) as u8,
        ]
    }
}
