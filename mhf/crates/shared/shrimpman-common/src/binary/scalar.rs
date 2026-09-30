use binrw::{BinRead, BinWrite};
use derive_more::{From, Into};
use jiff::Timestamp;

/// A boolean encoded as one unsigned byte.
#[derive(BinWrite, From)]
pub struct Bool8(#[bw(map = |&value| u8::from(value))] bool);

/// A Unix timestamp encoded as unsigned 32-bit seconds.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite, From, Into)]
pub struct UnixTimestamp32(
    #[br(try_map = |seconds: u32| Timestamp::new(i64::from(seconds), 0))]
    #[bw(try_map = |timestamp: &Timestamp| u32::try_from(timestamp.as_second()))]
    Timestamp,
);

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::{BinRead, BinWrite};

    use super::*;

    #[test]
    fn writes_bool8() {
        let mut output = Cursor::new(Vec::new());

        Bool8::from(true).write_be(&mut output).unwrap();
        Bool8::from(false).write_be(&mut output).unwrap();

        assert_eq!(output.into_inner(), [1, 0]);
    }

    #[test]
    fn reads_and_writes_unix_timestamp_32() {
        let timestamp = Timestamp::new(1_800_000_000, 0).unwrap();
        let mut output = Cursor::new(Vec::new());

        UnixTimestamp32::from(timestamp)
            .write_be(&mut output)
            .unwrap();
        assert_eq!(output.get_ref(), &1_800_000_000_u32.to_be_bytes());

        output.set_position(0);
        let decoded = UnixTimestamp32::read_be(&mut output).unwrap();

        assert_eq!(Timestamp::from(decoded), timestamp);
    }

    #[test]
    fn rejects_unrepresentable_unix_timestamp_32() {
        let timestamp = Timestamp::new(-1, 0).unwrap();
        let mut output = Cursor::new(Vec::new());

        assert!(
            UnixTimestamp32::from(timestamp)
                .write_be(&mut output)
                .is_err()
        );
    }
}
