use binrw::{BinRead, BinWrite, io::Cursor};

use crate::{DecodeStep, TransportError};

const CRYPT_HEADER_LEN: usize = 14;

/// Selects the body-length encoding used by the client generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FrameSizeMode {
    Legacy,
    #[default]
    Extended,
}

impl FrameSizeMode {
    pub const LEGACY_MAX_BODY_LEN: usize = u16::MAX as usize;
    pub const EXTENDED_MAX_BODY_LEN: usize = 0x0f_ffff;

    pub const fn max_body_len(self) -> usize {
        match self {
            Self::Legacy => Self::LEGACY_MAX_BODY_LEN,
            Self::Extended => Self::EXTENDED_MAX_BODY_LEN,
        }
    }

    fn encode_body_len(self, len: usize) -> Result<(u8, u16), TransportError> {
        if len > self.max_body_len() {
            return Err(TransportError::BodyLengthNotRepresentable { len, mode: self });
        }

        match self {
            Self::Legacy => Ok((0x03, len as u16)),
            Self::Extended => {
                let pf0 = (((len >> 12) & 0xf3) | 0x03) as u8;
                Ok((pf0, len as u16))
            }
        }
    }
}

/// The three integrity checks carried by a transport frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PacketChecksums([u16; 3]);

impl PacketChecksums {
    pub(crate) const fn new(check0: u16, check1: u16, check2: u16) -> Self {
        Self([check0, check1, check2])
    }

    pub const fn values(self) -> [u16; 3] {
        self.0
    }
}

/// The unencrypted 14-byte header preceding every encrypted body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub(crate) struct CryptHeader {
    pf0: u8,
    key_rotation_delta: u8,
    packet_number: u16,
    data_size: u16,
    previous_combined_check: u16,
    checksums: PacketChecksums,
}

impl CryptHeader {
    pub(crate) const ENCODED_LEN: usize = CRYPT_HEADER_LEN;

    fn body_len(self, mode: FrameSizeMode) -> Result<usize, TransportError> {
        match mode {
            FrameSizeMode::Legacy => Ok(usize::from(self.data_size)),
            FrameSizeMode::Extended => {
                let extension = self
                    .pf0
                    .checked_sub(0x03)
                    .ok_or(TransportError::InvalidPf0(self.pf0))?;

                Ok(usize::from(self.data_size) + usize::from(extension) * 0x1000)
            }
        }
    }

    pub(crate) const fn key_rotation_delta(self) -> u8 {
        self.key_rotation_delta
    }

    pub(crate) const fn checksums(self) -> PacketChecksums {
        self.checksums
    }

    pub(crate) fn for_body(
        mode: FrameSizeMode,
        body_len: usize,
        key_rotation_delta: u8,
        packet_number: u16,
        previous_combined_check: u16,
        checksums: PacketChecksums,
    ) -> Result<Self, TransportError> {
        let (pf0, data_size) = mode.encode_body_len(body_len)?;

        Ok(Self {
            pf0,
            key_rotation_delta,
            packet_number,
            data_size,
            previous_combined_check,
            checksums,
        })
    }
}

/// A complete transport frame before body decryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncryptedFrame {
    header: CryptHeader,
    body: Vec<u8>,
}

impl EncryptedFrame {
    pub(crate) fn new(header: CryptHeader, body: Vec<u8>) -> Self {
        Self { header, body }
    }

    pub(crate) fn into_parts(self) -> (CryptHeader, Vec<u8>) {
        (self.header, self.body)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FrameCodec {
    mode: FrameSizeMode,
}

impl FrameCodec {
    pub(crate) fn new(mode: FrameSizeMode) -> Self {
        Self { mode }
    }

    pub(crate) const fn mode(&self) -> FrameSizeMode {
        self.mode
    }

    pub(crate) fn validate_outbound_len(&self, len: usize) -> Result<(), TransportError> {
        self.mode.encode_body_len(len).map(|_| ())
    }

    pub(crate) fn decode(
        &self,
        input: &[u8],
    ) -> Result<DecodeStep<EncryptedFrame>, TransportError> {
        if input.len() < CRYPT_HEADER_LEN {
            return Ok(DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN,
            });
        }

        let mut reader = Cursor::new(&input[..CRYPT_HEADER_LEN]);
        let header = CryptHeader::read_be(&mut reader)?;
        let body_len = header.body_len(self.mode)?;

        if body_len > self.mode.max_body_len() {
            return Err(TransportError::BodyLengthNotRepresentable {
                len: body_len,
                mode: self.mode,
            });
        }

        let frame_len = CRYPT_HEADER_LEN + body_len;
        if input.len() < frame_len {
            return Ok(DecodeStep::NeedMore { needed: frame_len });
        }

        Ok(DecodeStep::Complete {
            value: EncryptedFrame {
                header,
                body: input[CRYPT_HEADER_LEN..frame_len].to_vec(),
            },
            consumed: frame_len,
        })
    }

    pub(crate) fn encode(
        &self,
        frame: &EncryptedFrame,
        output: &mut Vec<u8>,
    ) -> Result<(), TransportError> {
        let declared = frame.header.body_len(self.mode)?;
        let actual = frame.body.len();

        if declared != actual {
            return Err(TransportError::BodyLengthMismatch { declared, actual });
        }
        self.validate_outbound_len(actual)?;

        let mut writer = Cursor::new(Vec::with_capacity(CRYPT_HEADER_LEN + actual));
        frame.header.write_be(&mut writer)?;
        let mut encoded = writer.into_inner();
        encoded.extend_from_slice(&frame.body);
        output.extend_from_slice(&encoded);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_for_len(mode: FrameSizeMode, len: usize) -> CryptHeader {
        CryptHeader::for_body(mode, len, 3, 7, 11, PacketChecksums::new(13, 17, 19)).unwrap()
    }

    #[test]
    fn reads_and_writes_the_known_header_layout() {
        let bytes = [
            0x03, 0x03, 0x00, 0x01, 0x00, 0x0a, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05,
        ];
        let mut reader = Cursor::new(bytes);

        let header = CryptHeader::read_be(&mut reader).unwrap();

        assert_eq!(header.pf0, 0x03);
        assert_eq!(header.key_rotation_delta, 0x03);
        assert_eq!(header.packet_number, 1);
        assert_eq!(header.data_size, 10);
        assert_eq!(header.previous_combined_check, 2);
        assert_eq!(header.checksums, PacketChecksums::new(3, 4, 5));

        let mut writer = Cursor::new(Vec::new());
        header.write_be(&mut writer).unwrap();
        assert_eq!(writer.into_inner(), bytes);
    }

    #[test]
    fn extended_lengths_round_trip_at_boundaries() {
        for len in [0, 0x0fff, 0x1000, 0xffff, 0x1_0000, 0xa_1234, 0xf_ffff] {
            let header = header_for_len(FrameSizeMode::Extended, len);
            assert_eq!(header.body_len(FrameSizeMode::Extended).unwrap(), len);
        }
    }

    #[test]
    fn decoder_reports_total_bytes_needed_for_partial_frames() {
        let codec = FrameCodec::new(FrameSizeMode::Extended);
        assert_eq!(
            codec.decode(&[0; 5]).unwrap(),
            DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN,
            }
        );

        let frame = EncryptedFrame {
            header: header_for_len(FrameSizeMode::Extended, 4),
            body: vec![1, 2, 3, 4],
        };
        let mut encoded = Vec::new();
        codec.encode(&frame, &mut encoded).unwrap();

        assert_eq!(
            codec.decode(&encoded[..CRYPT_HEADER_LEN + 2]).unwrap(),
            DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN + 4,
            }
        );
    }

    #[test]
    fn decoder_consumes_only_one_frame() {
        let codec = FrameCodec::new(FrameSizeMode::Legacy);
        let frame = EncryptedFrame {
            header: header_for_len(FrameSizeMode::Legacy, 3),
            body: vec![1, 2, 3],
        };
        let mut encoded = Vec::new();
        codec.encode(&frame, &mut encoded).unwrap();
        encoded.extend_from_slice(&[9, 9]);

        let DecodeStep::Complete { value, consumed } = codec.decode(&encoded).unwrap() else {
            panic!("expected a complete frame");
        };

        assert_eq!(value, frame);
        assert_eq!(consumed, CRYPT_HEADER_LEN + 3);
    }
}
