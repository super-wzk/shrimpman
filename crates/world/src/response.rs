use std::io::Cursor;

use binrw::{
    BinRead, BinResult, BinWrite, Endian, Error as BinError,
    io::{Read, Seek, Write},
};
use bytes::Bytes;
use shrimpman_common::binary::{LengthEncoding, U16OrU32Length};

use crate::envelope::LandPacket;

pub(crate) const MSG_SYS_ACK: u16 = 0x0012;

/// Correlates one command response with its originating request.
#[repr(transparent)]
#[derive(Clone, Copy, BinRead, BinWrite)]
pub(crate) struct RequestHandle(u32);

impl RequestHandle {
    pub(crate) const fn from_slot(index: u16, generation: u16) -> Self {
        Self(((generation as u32) << 16) | index as u32)
    }

    pub(crate) const fn slot_index(self) -> usize {
        (self.0 & u16::MAX as u32) as usize
    }

    pub(crate) const fn generation(self) -> u16 {
        (self.0 >> 16) as u16
    }
}

impl From<RequestHandle> for u32 {
    fn from(value: RequestHandle) -> Self {
        value.0
    }
}

/// Error status carried by MSG_SYS_ACK.
#[repr(transparent)]
#[derive(Clone, Copy, BinRead, BinWrite)]
pub(crate) struct ResponseError(u8);

impl ResponseError {
    const SUCCESS: Self = Self(0);
    pub(crate) const ERROR: Self = Self(1);

    pub(crate) const fn is_success(self) -> bool {
        self.0 == 0
    }
}

/// One typed response carried by MSG_SYS_ACK.
pub(crate) struct CommandResponse<D> {
    handle: RequestHandle,
    error: ResponseError,
    data: D,
}

impl<D> CommandResponse<D> {
    pub(crate) const fn success(handle: RequestHandle, data: D) -> Self {
        Self {
            handle,
            error: ResponseError::SUCCESS,
            data,
        }
    }

    pub(crate) const fn handle(&self) -> RequestHandle {
        self.handle
    }

    pub(crate) fn into_parts(self) -> (RequestHandle, ResponseError, D) {
        (self.handle, self.error, self.data)
    }
}

impl CommandResponse<Value> {
    pub(crate) const fn failure(handle: RequestHandle, error: ResponseError) -> Self {
        Self {
            handle,
            error,
            data: Value(0),
        }
    }
}

impl<D> LandPacket for CommandResponse<D>
where
    D: Send + 'static,
{
    const OPCODE: u16 = MSG_SYS_ACK;
}

/// A fixed four-byte response value.
pub(crate) struct Value(pub(crate) u32);

/// A length-prefixed response decoded as `T`.
#[cfg_attr(not(test), allow(dead_code, reason = "buffered response API"))]
pub(crate) struct Buffered<T>(pub(crate) T);

trait EncodeResponse {
    const BUFFERED: bool;

    fn encode(&self, endian: Endian) -> BinResult<Vec<u8>>;
}

impl EncodeResponse for Value {
    const BUFFERED: bool = false;

    fn encode(&self, endian: Endian) -> BinResult<Vec<u8>> {
        encode_value(&self.0, endian)
    }
}

impl<T> EncodeResponse for Buffered<T>
where
    T: for<'args> BinWrite<Args<'args> = ()>,
{
    const BUFFERED: bool = true;

    fn encode(&self, endian: Endian) -> BinResult<Vec<u8>> {
        encode_value(&self.0, endian)
    }
}

impl<D> BinWrite for CommandResponse<D>
where
    D: EncodeResponse,
{
    type Args<'args> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let data = self.data.encode(endian)?;
        self.handle.write_options(writer, endian, ())?;
        u8::from(D::BUFFERED).write_options(writer, endian, ())?;
        self.error.write_options(writer, endian, ())?;
        if D::BUFFERED {
            U16OrU32Length::write_length(data.len(), writer, endian)?;
        } else {
            0_u16.write_options(writer, endian, ())?;
        }
        writer.write_all(&data)?;
        Ok(())
    }
}

fn encode_value<T>(value: &T, endian: Endian) -> BinResult<Vec<u8>>
where
    T: for<'args> BinWrite<Args<'args> = ()>,
{
    let mut output = Cursor::new(Vec::new());
    value.write_options(&mut output, endian, ())?;
    Ok(output.into_inner())
}

/// Runtime response representation decoded before its request type is known.
pub(crate) enum ResponseBody {
    Value(u32),
    #[cfg_attr(not(test), allow(dead_code, reason = "buffered response API"))]
    Buffer(Bytes),
}

pub(crate) type WireResponse = CommandResponse<ResponseBody>;

impl BinRead for WireResponse {
    type Args<'args> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let handle = RequestHandle::read_options(reader, endian, ())?;
        let buffered = u8::read_options(reader, endian, ())? != 0;
        let error = ResponseError::read_options(reader, endian, ())?;
        let length = U16OrU32Length::read_length(reader, endian)?;
        let data = if buffered {
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes)?;
            ResponseBody::Buffer(Bytes::from(bytes))
        } else {
            if length != 0 {
                return Err(BinError::AssertFail {
                    pos: reader.stream_position()?,
                    message: format!(
                        "value command response declared a non-zero buffer length of {length}"
                    ),
                });
            }
            ResponseBody::Value(u32::read_options(reader, endian, ())?)
        };

        Ok(Self {
            handle,
            error,
            data,
        })
    }
}

/// Decodes the response representation selected by `exchange.request::<Want>`.
pub(crate) trait ExpectedResponse {
    type Output;

    fn decode(data: ResponseBody) -> BinResult<Self::Output>;
}

impl ExpectedResponse for Value {
    type Output = u32;

    fn decode(data: ResponseBody) -> BinResult<Self::Output> {
        let ResponseBody::Value(value) = data else {
            return Err(unexpected_response("value", "buffer"));
        };
        Ok(value)
    }
}

impl<T> ExpectedResponse for Buffered<T>
where
    T: for<'args> BinRead<Args<'args> = ()>,
{
    type Output = T;

    fn decode(data: ResponseBody) -> BinResult<Self::Output> {
        let ResponseBody::Buffer(bytes) = data else {
            return Err(unexpected_response("buffer", "value"));
        };
        let length = bytes.len() as u64;
        let mut input = Cursor::new(bytes);
        let value = T::read_be(&mut input)?;
        if input.position() != length {
            return Err(BinError::AssertFail {
                pos: input.position(),
                message: format!(
                    "command response left {} trailing bytes",
                    length - input.position()
                ),
            });
        }
        Ok(value)
    }
}

fn unexpected_response(expected: &str, actual: &str) -> BinError {
    BinError::AssertFail {
        pos: 0,
        message: format!("expected a {expected} command response, received a {actual} response"),
    }
}

/// Adds the connection-generated request handle before a typed command payload.
pub(crate) struct RequestEnvelope<P> {
    pub(crate) handle: RequestHandle,
    pub(crate) payload: P,
}

impl<P> LandPacket for RequestEnvelope<P>
where
    P: LandPacket,
{
    const OPCODE: u16 = P::OPCODE;
}

impl<P> BinWrite for RequestEnvelope<P>
where
    P: for<'args> BinWrite<Args<'args> = ()>,
{
    type Args<'args> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        self.handle.write_options(writer, endian, ())?;
        self.payload.write_options(writer, endian, ())
    }
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite};

    use super::*;

    #[test]
    fn writes_a_simple_command_response() {
        let response = CommandResponse::success(RequestHandle(7), Value(1_800_000_000));
        let mut output = Cursor::new(Vec::new());

        response.write_be(&mut output).unwrap();

        assert_eq!(
            output.get_ref(),
            &[
                0, 0, 0, 7, // request handle
                0, // value response
                0, // success
                0, 0, // value responses carry a zero buffer length
                0x6b, 0x49, 0xd2, 0x00, // value
            ]
        );

        output.set_position(0);
        let response = WireResponse::read_be(&mut output).unwrap();
        let (_, error, data) = response.into_parts();
        assert!(error.is_success());
        assert_eq!(Value::decode(data).unwrap(), 1_800_000_000);
    }

    #[test]
    fn writes_a_simple_command_failure() {
        let response = CommandResponse::failure(RequestHandle(7), ResponseError::ERROR);
        let mut output = Cursor::new(Vec::new());

        response.write_be(&mut output).unwrap();

        assert_eq!(
            output.into_inner(),
            [
                0, 0, 0, 7, // request handle
                0, // value response
                1, // generic error
                0, 0, // value responses carry a zero buffer length
                0, 0, 0, 0, // failure responses carry a zero value
            ]
        );
    }

    #[test]
    fn round_trips_a_buffered_command_response() {
        let response = CommandResponse::success(RequestHandle(0x0001_0002), Buffered(0x1234_u16));
        let mut output = Cursor::new(Vec::new());
        response.write_be(&mut output).unwrap();
        assert_eq!(
            output.get_ref(),
            &[
                0, 1, 0, 2, // request handle
                1, // buffered response
                0, // success
                0, 2, // buffer length
                0x12, 0x34,
            ]
        );

        output.set_position(0);
        let response = WireResponse::read_be(&mut output).unwrap();
        let (handle, error, data) = response.into_parts();

        assert_eq!(u32::from(handle), 0x0001_0002);
        assert!(error.is_success());
        assert_eq!(Buffered::<u16>::decode(data).unwrap(), 0x1234);
    }
}
