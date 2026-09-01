use bytes::{Buf, Bytes, BytesMut};
use nu_pretty_hex::PrettyHex;
use tokio_util::codec::{Decoder, Encoder};
use tracing::debug;

use crate::{
    DecodeStep,
    crypto::{DecryptState, EncryptState},
    error::TransportError,
    frame::{CryptHeader, FrameCodec},
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

impl Decoder for MhfTransportCodec {
    type Item = Bytes;
    type Error = TransportError;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (header, consumed) = match self.frame.decode(source.as_ref())? {
            DecodeStep::NeedMore { needed } => {
                source.reserve(needed.saturating_sub(source.len()));
                return Ok(None);
            }
            DecodeStep::Complete { value, consumed } => {
                let (header, _) = value.into_parts();
                (header, consumed)
            }
        };

        let mut body = source.split_to(consumed);
        body.advance(CryptHeader::ENCODED_LEN);

        // A checksum error is terminal for the frame, but cipher state remains
        // unchanged because decrypt_in_place commits it only after validation.
        self.inbound.decrypt_in_place(header, body.as_mut())?;
        debug!(payload = ?body.hex_dump(), "Received");
        Ok(Some(body.freeze()))
    }
}

impl Encoder<Bytes> for MhfTransportCodec {
    type Error = TransportError;

    fn encode(&mut self, payload: Bytes, destination: &mut BytesMut) -> Result<(), Self::Error> {
        let body_len = payload.len();
        self.frame.validate_outbound_len(body_len)?;

        let frame_start = destination.len();
        destination.reserve(CryptHeader::ENCODED_LEN + body_len);
        destination.resize(frame_start + CryptHeader::ENCODED_LEN, 0);

        // Encrypt directly in the final transport buffer. Both destination and
        // cipher state are rolled back if header construction ever fails.
        let mut next_outbound = self.outbound;
        let encoded = (|| {
            let encrypted = next_outbound.encrypt_into(payload.as_ref(), destination);
            let header = CryptHeader::for_body(
                body_len,
                encrypted.key_rotation_delta,
                encrypted.packet_number,
                encrypted.previous_combined_check,
                encrypted.checksums,
            )?;
            let header_end = frame_start + CryptHeader::ENCODED_LEN;
            self.frame.encode_header(
                header,
                body_len,
                (&mut destination[frame_start..header_end])
                    .try_into()
                    .expect("header output has the fixed transport header length"),
            )
        })();

        if let Err(error) = encoded {
            destination.truncate(frame_start);
            return Err(error);
        }

        self.outbound = next_outbound;
        debug!(payload = ?payload.hex_dump(), "Sent");
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
    fn checksum_failures_consume_the_frame_without_advancing_cipher_state() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let mut frame = BytesMut::new();
        sender
            .encode(Bytes::from_static(b"payload"), &mut frame)
            .unwrap();

        let mut corrupted = frame.clone();
        corrupted[CryptHeader::ENCODED_LEN] ^= 0xff;
        assert!(matches!(
            receiver.decode(&mut corrupted),
            Err(TransportError::ChecksumMismatch { .. })
        ));
        assert!(corrupted.is_empty());

        let value = receiver.decode(&mut frame).unwrap().unwrap();
        assert_eq!(value.as_ref(), b"payload");
    }

    #[test]
    fn successful_decryption_reuses_the_inbound_body_storage() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let mut frame = BytesMut::new();
        sender
            .encode(Bytes::from_static(b"payload"), &mut frame)
            .unwrap();
        let body_ptr = frame[CryptHeader::ENCODED_LEN..].as_ptr();

        let value = receiver.decode(&mut frame).unwrap().unwrap();

        assert_eq!(value.as_ref(), b"payload");
        assert_eq!(value.as_ptr(), body_ptr);
        assert!(frame.is_empty());
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

    #[test]
    fn writes_frames_into_the_existing_destination_allocation() {
        let mut sender = MhfTransportCodec::default();
        let mut receiver = MhfTransportCodec::default();
        let payload = Bytes::from_static(b"payload");
        let mut output = BytesMut::with_capacity(1 + CryptHeader::ENCODED_LEN + payload.len());
        output.extend_from_slice(&[0xaa]);
        let allocation = output.as_ptr();

        sender.encode(payload, &mut output).unwrap();

        assert_eq!(output.as_ptr(), allocation);
        assert_eq!(output[0], 0xaa);
        let mut frame = output.split_off(1);
        let decoded = receiver.decode(&mut frame).unwrap().unwrap();
        assert_eq!(decoded.as_ref(), b"payload");
        assert!(frame.is_empty());
    }
}
