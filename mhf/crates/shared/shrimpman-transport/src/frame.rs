use binrw::{BinRead, BinWrite, io::Cursor};

use crate::{DecodeStep, TransportError};

const CRYPT_HEADER_LEN: usize = 14;

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
    const MAX_BODY_LEN: usize = 0x0f_ffff;

    fn body_len(self) -> Result<usize, TransportError> {
        let extension = self
            .pf0
            .checked_sub(0x03)
            .ok_or(TransportError::InvalidPf0(self.pf0))?;

        Ok(usize::from(self.data_size) + usize::from(extension) * 0x1000)
    }

    fn encode_body_len(len: usize) -> Result<(u8, u16), TransportError> {
        if len > Self::MAX_BODY_LEN {
            return Err(TransportError::BodyLengthNotRepresentable { len });
        }

        let pf0 = (((len >> 12) & 0xf3) | 0x03) as u8;
        Ok((pf0, len as u16))
    }

    pub(crate) const fn key_rotation_delta(self) -> u8 {
        self.key_rotation_delta
    }

    pub(crate) const fn checksums(self) -> PacketChecksums {
        self.checksums
    }

    pub(crate) fn for_body(
        body_len: usize,
        key_rotation_delta: u8,
        packet_number: u16,
        previous_combined_check: u16,
        checksums: PacketChecksums,
    ) -> Result<Self, TransportError> {
        let (pf0, data_size) = Self::encode_body_len(body_len)?;

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

/// A complete transport frame with an owned or borrowed encrypted body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncryptedFrame<Body> {
    header: CryptHeader,
    body: Body,
}

impl<Body> EncryptedFrame<Body> {
    pub(crate) fn into_parts(self) -> (CryptHeader, Body) {
        (self.header, self.body)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FrameCodec;

impl FrameCodec {
    pub(crate) fn validate_outbound_len(&self, len: usize) -> Result<(), TransportError> {
        CryptHeader::encode_body_len(len).map(|_| ())
    }

    pub(crate) fn decode<'input>(
        &self,
        input: &'input [u8],
    ) -> Result<DecodeStep<EncryptedFrame<&'input [u8]>>, TransportError> {
        if input.len() < CRYPT_HEADER_LEN {
            return Ok(DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN,
            });
        }

        let mut reader = Cursor::new(&input[..CRYPT_HEADER_LEN]);
        let header = CryptHeader::read_be(&mut reader)?;
        let body_len = header.body_len()?;

        if body_len > CryptHeader::MAX_BODY_LEN {
            return Err(TransportError::BodyLengthNotRepresentable { len: body_len });
        }

        let frame_len = CRYPT_HEADER_LEN + body_len;
        if input.len() < frame_len {
            return Ok(DecodeStep::NeedMore { needed: frame_len });
        }

        Ok(DecodeStep::Complete {
            value: EncryptedFrame {
                header,
                body: &input[CRYPT_HEADER_LEN..frame_len],
            },
            consumed: frame_len,
        })
    }

    pub(crate) fn encode_header(
        &self,
        header: CryptHeader,
        body_len: usize,
        output: &mut [u8; CRYPT_HEADER_LEN],
    ) -> Result<(), TransportError> {
        let declared = header.body_len()?;

        if declared != body_len {
            return Err(TransportError::BodyLengthMismatch {
                declared,
                actual: body_len,
            });
        }
        self.validate_outbound_len(body_len)?;

        let mut writer = Cursor::new(&mut output[..]);
        header.write_be(&mut writer)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_for_len(len: usize) -> CryptHeader {
        CryptHeader::for_body(len, 3, 7, 11, PacketChecksums::new(13, 17, 19)).unwrap()
    }

    fn encode_frame(codec: &FrameCodec, frame: &EncryptedFrame<Vec<u8>>) -> Vec<u8> {
        let mut encoded = vec![0; CRYPT_HEADER_LEN + frame.body.len()];
        let (header, body) = encoded.split_at_mut(CRYPT_HEADER_LEN);
        body.copy_from_slice(&frame.body);
        codec
            .encode_header(frame.header, frame.body.len(), header.try_into().unwrap())
            .unwrap();
        encoded
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

        let mut encoded = [0; CRYPT_HEADER_LEN];
        FrameCodec.encode_header(header, 10, &mut encoded).unwrap();
        assert_eq!(encoded, bytes);
    }

    #[test]
    fn body_lengths_round_trip_at_boundaries() {
        for len in [0, 0x0fff, 0x1000, 0xffff, 0x1_0000, 0xa_1234, 0xf_ffff] {
            let header = header_for_len(len);
            assert_eq!(header.body_len().unwrap(), len);
        }
    }

    #[test]
    fn decoder_reports_total_bytes_needed_for_partial_frames() {
        let codec = FrameCodec;
        assert_eq!(
            codec.decode(&[0; 5]).unwrap(),
            DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN,
            }
        );

        let frame = EncryptedFrame {
            header: header_for_len(4),
            body: vec![1, 2, 3, 4],
        };
        let encoded = encode_frame(&codec, &frame);

        assert_eq!(
            codec.decode(&encoded[..CRYPT_HEADER_LEN + 2]).unwrap(),
            DecodeStep::NeedMore {
                needed: CRYPT_HEADER_LEN + 4,
            }
        );
    }

    #[test]
    fn decoder_consumes_only_one_frame() {
        let codec = FrameCodec;
        let frame = EncryptedFrame {
            header: header_for_len(3),
            body: vec![1, 2, 3],
        };
        let mut encoded = encode_frame(&codec, &frame);
        encoded.extend_from_slice(&[9, 9]);

        let DecodeStep::Complete { value, consumed } = codec.decode(&encoded).unwrap() else {
            panic!("expected a complete frame");
        };

        assert_eq!(value.header, frame.header);
        assert_eq!(value.body, frame.body.as_slice());
        assert_eq!(consumed, CRYPT_HEADER_LEN + 3);
    }
}
