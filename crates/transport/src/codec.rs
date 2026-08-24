use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    DecodeStep,
    crypto::{DecryptState, EncryptState},
    error::TransportError,
    frame::{CryptHeader, EncryptedFrame, FrameCodec},
};

/// Stateful codec for the encrypted MHF transport format.
///
/// The codec handles only the common encrypted transport envelope. The
/// returned bytes remain opaque to this crate and are decoded by a service
/// crate. It can be used with [`tokio_util::codec::Framed`] as both a decoder
/// and an encoder of [`Bytes`].
#[derive(Debug, Clone, Default)]
pub(crate) struct MhfTransportCodec {
    frame: FrameCodec,
    inbound: DecryptState,
    outbound: EncryptState,
}

impl MhfTransportCodec {
    fn decode_frame(&mut self, input: &[u8]) -> Result<DecodeStep<Vec<u8>>, TransportError> {
        let (frame, consumed) = match self.frame.decode(input)? {
            DecodeStep::NeedMore { needed } => {
                return Ok(DecodeStep::NeedMore { needed });
            }
            DecodeStep::Complete { value, consumed } => (value, consumed),
        };

        // Only commit cipher state after all integrity checks succeed.
        let (header, body) = frame.into_parts();
        let mut next_inbound = self.inbound;
        let payload = next_inbound.decrypt(header, body)?;
        self.inbound = next_inbound;

        Ok(DecodeStep::Complete {
            value: payload,
            consumed,
        })
    }

    fn encode_frame(&mut self, payload: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.frame.validate_outbound_len(payload.len())?;

        // Only commit cipher state after the complete frame is encoded.
        let mut next_outbound = self.outbound;
        let encrypted = next_outbound.encrypt(payload);
        let body_len = encrypted.data.len();
        let header = CryptHeader::for_body(
            body_len,
            encrypted.key_rotation_delta,
            encrypted.packet_number,
            encrypted.previous_combined_check,
            encrypted.checksums,
        )?;
        let frame = EncryptedFrame::new(header, encrypted.data);
        let mut encoded = Vec::with_capacity(CryptHeader::ENCODED_LEN + body_len);
        self.frame.encode(&frame, &mut encoded)?;

        self.outbound = next_outbound;
        Ok(encoded)
    }
}

impl Decoder for MhfTransportCodec {
    type Item = Bytes;
    type Error = TransportError;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode_frame(source.as_ref())? {
            DecodeStep::NeedMore { needed } => {
                source.reserve(needed.saturating_sub(source.len()));
                Ok(None)
            }
            DecodeStep::Complete { value, consumed } => {
                source.advance(consumed);
                Ok(Some(value.into()))
            }
        }
    }
}

impl Encoder<Bytes> for MhfTransportCodec {
    type Error = TransportError;

    fn encode(&mut self, payload: Bytes, destination: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = self.encode_frame(payload.as_ref())?;
        destination.extend_from_slice(&frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Bytes, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

    use super::MhfTransportCodec;
    use crate::{error::TransportError, frame::CryptHeader};

    #[test]
    fn round_trips_multiple_packets_and_preserves_stream_boundaries() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let payloads: [&[u8]; 3] = [b"first", b"", b"third payload"];
        let mut stream = BytesMut::new();

        for payload in payloads {
            sender
                .encode(Bytes::copy_from_slice(payload), &mut stream)
                .unwrap();
        }

        for expected in payloads {
            let value = receiver.decode(&mut stream).unwrap().unwrap();
            assert_eq!(value.as_ref(), expected);
        }
        assert!(stream.is_empty());
    }

    #[test]
    fn incomplete_frames_do_not_advance_cipher_state() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let mut frame = BytesMut::new();
        sender
            .encode(Bytes::from_static(b"payload"), &mut frame)
            .unwrap();
        let mut partial = BytesMut::from(&frame[..5]);

        assert_eq!(receiver.decode(&mut partial).unwrap(), None);
        assert_eq!(partial.as_ref(), &frame[..5]);
        assert!(partial.capacity() >= CryptHeader::ENCODED_LEN);

        let value = receiver.decode(&mut frame).unwrap().unwrap();
        assert_eq!(value.as_ref(), b"payload");
    }

    #[test]
    fn checksum_failures_do_not_advance_cipher_state() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let mut frame = BytesMut::new();
        sender
            .encode(Bytes::from_static(b"payload"), &mut frame)
            .unwrap();

        let mut corrupted = frame.clone();
        corrupted[CryptHeader::ENCODED_LEN] ^= 0xff;
        let original = corrupted.clone();
        assert!(matches!(
            receiver.decode(&mut corrupted),
            Err(TransportError::ChecksumMismatch { .. })
        ));
        assert_eq!(corrupted, original);

        let value = receiver.decode(&mut frame).unwrap().unwrap();
        assert_eq!(value.as_ref(), b"payload");
    }

    #[test]
    fn round_trips_payloads_larger_than_u16() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let payload = vec![0xa5; usize::from(u16::MAX) + 17];
        let mut frame = BytesMut::new();

        sender
            .encode(Bytes::copy_from_slice(&payload), &mut frame)
            .unwrap();
        let value = receiver.decode(&mut frame).unwrap().unwrap();

        assert_eq!(value.as_ref(), payload.as_slice());
        assert!(frame.is_empty());
    }

    #[test]
    fn unrepresentable_payloads_leave_output_unchanged() {
        let mut codec = MhfTransportCodec::default();
        let mut output = BytesMut::from(&[0xaa][..]);
        let payload_len = 0x10_0000;
        let payload = Bytes::from(vec![0; payload_len]);

        assert!(matches!(
            codec.encode(payload, &mut output),
            Err(TransportError::BodyLengthNotRepresentable { len }) if len == payload_len
        ));
        assert_eq!(output.as_ref(), &[0xaa]);
    }
}
