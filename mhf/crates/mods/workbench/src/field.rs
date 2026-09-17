//! Byte-backed field types. Display labels never determine storage semantics.

use std::{borrow::Cow, fmt, fmt::Write as _, ops::Range, sync::Arc};

use mhf_resource::binary::{self, BinaryValue, ValueKind};
pub use mhf_resource::binary::{Endian, ScalarType};

/// A field's storage identity is independent of the node that displays it.
/// DAT names and referenced effect definitions may live outside that node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub buffer: usize,
    pub range: Range<usize>,
    pub format: FieldType,
    pub endian: Endian,
}

impl Binding {
    pub fn decode(&self, bytes: &[u8]) -> Result<String, String> {
        if bytes.len() != self.range.len() {
            return Err("字段字节数与绑定范围不符".into());
        }
        self.format.decode_endian(bytes, self.endian)
    }

    pub fn encode(&self, original: &[u8], input: &str) -> Result<Vec<u8>, String> {
        if original.len() != self.range.len() {
            return Err("字段字节数与绑定范围不符".into());
        }
        self.format.encode_endian(original, input, self.endian)
    }

    pub fn bytes<'a>(&self, buffers: &'a [Arc<[u8]>]) -> Result<&'a [u8], String> {
        buffers
            .get(self.buffer)
            .ok_or("字段数据层不存在")?
            .get(self.range.clone())
            .ok_or_else(|| "字段范围超出数据层".into())
    }

    pub fn read(&self, buffers: &[Arc<[u8]>]) -> Result<String, String> {
        self.decode(self.bytes(buffers)?)
    }

    /// Builds a checked edit without mutating the document. Byte equality also
    /// suppresses equivalent numeric input and unchanged noncanonical floats.
    pub fn write(&self, buffers: &[Arc<[u8]>], input: &str) -> Result<Option<Patch>, String> {
        let before = self.bytes(buffers)?;
        let after = self.encode(before, input)?;
        Ok((before != after).then(|| Patch {
            binding: self.clone(),
            before: before.to_vec(),
            after,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    pub binding: Binding,
    /// Used by the editor to reject patches against a replaced document.
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub value: String,
    /// Value legend for this field, shown with the name's hover text. Only
    /// confirmed interpretations belong here; raw offsets stay in `name`.
    pub note: Option<&'static str>,
    pub binding: Binding,
    pub writable: bool,
}

impl Field {
    pub fn from_binary<T: BinaryValue + fmt::Debug>(
        name: impl Into<String>,
        buffer: usize,
        source: binary::Field<'_, T>,
    ) -> Self {
        Self {
            name: name.into(),
            value: format!("{:?}", source.value),
            note: None,
            writable: !source.range.is_empty(),
            binding: Binding {
                buffer,
                range: source.range,
                format: FieldType::from_binary(T::KIND),
                endian: source.endian,
            },
        }
    }

    pub fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }

    pub fn read(&self, buffers: &[Arc<[u8]>]) -> Result<String, String> {
        self.binding.read(buffers)
    }

    pub fn write(&self, buffers: &[Arc<[u8]>], input: &str) -> Result<Option<Patch>, String> {
        if !self.writable {
            return Err("此字段只读，请替换完整资源或显式编辑原始字节".into());
        }
        self.binding.write(buffers, input)
    }
}

fn flag_storage(scalar: ScalarType) -> Result<ScalarType, String> {
    match scalar {
        ScalarType::U8 | ScalarType::U16 | ScalarType::U32 | ScalarType::U64 => Ok(scalar),
        _ => Err("位标志必须使用无符号整数存储类型".into()),
    }
}

fn decode_scalar(scalar: ScalarType, bytes: &[u8], endian: Endian) -> Result<String, String> {
    macro_rules! read {
        ($ty:ty) => {
            <$ty as BinaryValue>::decode(bytes, endian).map(|value| value.to_string())
        };
    }
    match scalar {
        ScalarType::U8 => read!(u8),
        ScalarType::U16 => read!(u16),
        ScalarType::U32 => read!(u32),
        ScalarType::U64 => read!(u64),
        ScalarType::I8 => read!(i8),
        ScalarType::I16 => read!(i16),
        ScalarType::I32 => read!(i32),
        ScalarType::I64 => read!(i64),
        ScalarType::F32 => read!(f32),
        ScalarType::F64 => read!(f64),
    }
    .map_err(|error| error.to_string())
}

fn encode_scalar(
    scalar: ScalarType,
    output: &mut [u8],
    input: &str,
    endian: Endian,
) -> Result<(), String> {
    let input = input.trim();
    // Preserve NaN payloads and signed zero when a component was not changed.
    if matches!(scalar, ScalarType::F32 | ScalarType::F64)
        && decode_scalar(scalar, output, endian)? == input
    {
        return Ok(());
    }
    let range_error = || format!("输入超出 {scalar:?} 范围或格式不正确");
    macro_rules! integer {
        ($ty:ty) => {
            <$ty>::try_from(parse_integer(input)?)
                .map_err(|_| range_error())?
                .encode(output, endian)
        };
    }
    macro_rules! float {
        ($ty:ty) => {{
            let value = input.parse::<$ty>().map_err(|_| range_error())?;
            if value.is_infinite()
                && !matches!(
                    input.to_ascii_lowercase().as_str(),
                    "inf" | "+inf" | "-inf" | "infinity" | "+infinity" | "-infinity"
                )
            {
                return Err(range_error());
            }
            value.encode(output, endian)
        }};
    }
    match scalar {
        ScalarType::U8 => integer!(u8),
        ScalarType::U16 => integer!(u16),
        ScalarType::U32 => integer!(u32),
        ScalarType::U64 => integer!(u64),
        ScalarType::I8 => integer!(i8),
        ScalarType::I16 => integer!(i16),
        ScalarType::I32 => integer!(i32),
        ScalarType::I64 => integer!(i64),
        ScalarType::F32 => float!(f32),
        ScalarType::F64 => float!(f64),
    }
    .map_err(|error| error.to_string())
}

fn parse_integer(input: &str) -> Result<i128, String> {
    let input = input.trim();
    let input = if input.contains('_') {
        Cow::Owned(input.replace('_', ""))
    } else {
        Cow::Borrowed(input)
    };
    let (negative, digits) = input
        .strip_prefix('-')
        .map_or((false, input.as_ref()), |v| (true, v));
    let digits = digits.strip_prefix('+').unwrap_or(digits);
    let (radix, digits) = if let Some(v) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, v)
    } else if let Some(v) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, v)
    } else {
        (10, digits)
    };
    i128::from_str_radix(digits, radix)
        .map(|v| if negative { -v } else { v })
        .map_err(|_| "请输入十进制整数、0x 十六进制或 0b 二进制整数".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    ShiftJis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    ReadOnly,
    Scalar(ScalarType),
    Array(ScalarType),
    Flags(ScalarType),
    Color {
        alpha: bool,
    },
    Bytes,
    Text {
        encoding: TextEncoding,
        terminated: bool,
    },
}

impl FieldType {
    fn from_binary(kind: ValueKind) -> Self {
        match kind {
            ValueKind::Scalar(scalar) => Self::Scalar(scalar),
            ValueKind::Array { element, .. } => match Self::from_binary(*element) {
                Self::Scalar(scalar) | Self::Array(scalar) => Self::Array(scalar),
                _ => unreachable!("binary values contain only typed scalars and arrays"),
            },
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Result<String, String> {
        self.decode_endian(bytes, Endian::Little)
    }

    fn decode_endian(self, bytes: &[u8], endian: Endian) -> Result<String, String> {
        match self {
            Self::ReadOnly => Err("此字段是派生说明，不能直接编辑".into()),
            Self::Scalar(scalar) => decode_scalar(scalar, bytes, endian),
            Self::Flags(scalar) => {
                let value = decode_scalar(flag_storage(scalar)?, bytes, endian)?;
                Ok(format!("0x{:X}", parse_integer(&value)?))
            }
            Self::Array(scalar) => {
                if !bytes.len().is_multiple_of(scalar.size()) {
                    return Err("数组长度与元素类型不符".into());
                }
                bytes
                    .chunks_exact(scalar.size())
                    .map(|bytes| decode_scalar(scalar, bytes, endian))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|values| values.join(", "))
            }
            Self::Color { alpha } => {
                if bytes.len() != if alpha { 4 } else { 3 } {
                    return Err("颜色长度不正确".into());
                }
                Self::Array(ScalarType::U8).decode(bytes)
            }
            Self::Bytes => {
                let capacity = bytes.len().checked_mul(3).ok_or("十六进制文本长度溢出")?;
                let mut text = String::new();
                text.try_reserve(capacity)
                    .map_err(|_| "无法分配十六进制文本")?;
                for (index, byte) in bytes.iter().enumerate() {
                    if index != 0 {
                        text.push(' ');
                    }
                    write!(text, "{byte:02X}").expect("writing to a String cannot fail");
                }
                Ok(text)
            }
            Self::Text {
                encoding,
                terminated,
            } => decode_text(bytes, encoding, terminated),
        }
    }

    /// Encode into the existing field capacity. Only the caller-selected field
    /// is written; untouched bytes, including noncanonical floats, stay intact.
    pub fn encode(self, original: &[u8], input: &str) -> Result<Vec<u8>, String> {
        self.encode_endian(original, input, Endian::Little)
    }

    fn encode_endian(
        self,
        original: &[u8],
        input: &str,
        endian: Endian,
    ) -> Result<Vec<u8>, String> {
        match self {
            Self::ReadOnly => Err("此字段是派生说明，不能直接编辑".into()),
            Self::Scalar(scalar) | Self::Flags(scalar) => {
                if matches!(self, Self::Flags(_)) {
                    flag_storage(scalar)?;
                }
                let mut output = original.to_vec();
                encode_scalar(scalar, &mut output, input, endian)?;
                Ok(output)
            }
            Self::Array(scalar) => {
                if !original.len().is_multiple_of(scalar.size()) {
                    return Err("数组长度与元素类型不符".into());
                }
                let values = input
                    .split(|c: char| c.is_whitespace() || matches!(c, ',' | '[' | ']' | ';'))
                    .filter(|value| !value.is_empty());
                if values.clone().count() != original.len() / scalar.size() {
                    return Err(format!(
                        "需要 {} 个数组元素",
                        original.len() / scalar.size()
                    ));
                }
                let mut output = original.to_vec();
                for (bytes, value) in output.chunks_exact_mut(scalar.size()).zip(values) {
                    encode_scalar(scalar, bytes, value, endian)?;
                }
                Ok(output)
            }
            Self::Color { .. } => {
                self.decode(original)?;
                Self::Array(ScalarType::U8).encode(original, input)
            }
            Self::Bytes => {
                let digits: String = input.chars().filter(|c| !c.is_whitespace()).collect();
                if digits.len() != original.len() * 2
                    || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(format!("需要 {} 个十六进制字节", original.len()));
                }
                digits
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| {
                        u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                            .map_err(|_| "十六进制字节不正确".into())
                    })
                    .collect()
            }
            Self::Text {
                encoding,
                terminated,
            } => {
                if self.decode_endian(original, endian).as_deref() == Ok(input) {
                    return Ok(original.to_vec());
                }
                encode_text(original.len(), input, encoding, terminated)
            }
        }
    }
}

fn decode_text(bytes: &[u8], encoding: TextEncoding, terminated: bool) -> Result<String, String> {
    if matches!(encoding, TextEncoding::Utf16Le | TextEncoding::Utf16Be) {
        if !bytes.len().is_multiple_of(2) {
            return Err("UTF-16 字段长度必须为偶数".into());
        }
        let words = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| {
                if encoding == TextEncoding::Utf16Le {
                    u16::from_le_bytes([v[0], v[1]])
                } else {
                    u16::from_be_bytes([v[0], v[1]])
                }
            })
            .take_while(|&v| !terminated || v != 0);
        return char::decode_utf16(words)
            .collect::<Result<String, _>>()
            .map_err(|_| "UTF-16 文本无效；可使用二进制编辑".into());
    }
    let bytes = if terminated {
        &bytes[..bytes.iter().position(|&v| v == 0).unwrap_or(bytes.len())]
    } else {
        bytes
    };
    match encoding {
        TextEncoding::Utf8 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "UTF-8 文本无效；可使用二进制编辑".into()),
        TextEncoding::ShiftJis => {
            let (text, errors) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
            if errors {
                Err("CP932 文本无效；可使用二进制编辑".into())
            } else {
                Ok(text.into_owned())
            }
        }
        _ => unreachable!(),
    }
}

fn encode_text(
    capacity: usize,
    input: &str,
    encoding: TextEncoding,
    terminated: bool,
) -> Result<Vec<u8>, String> {
    if terminated && input.contains('\0') {
        return Err("文本不能包含内嵌 NUL".into());
    }
    let mut bytes = match encoding {
        TextEncoding::Utf8 => input.as_bytes().to_vec(),
        TextEncoding::ShiftJis => {
            let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(input);
            if errors {
                return Err("文本包含 CP932 无法编码的字符".into());
            }
            bytes.into_owned()
        }
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            if !capacity.is_multiple_of(2) {
                return Err("UTF-16 字段长度必须为偶数".into());
            }
            input
                .encode_utf16()
                .flat_map(|v| {
                    if encoding == TextEncoding::Utf16Le {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    }
                })
                .collect()
        }
    };
    let terminator = if !terminated {
        0
    } else if matches!(encoding, TextEncoding::Utf16Le | TextEncoding::Utf16Be) {
        2
    } else {
        1
    };
    if bytes.len().saturating_add(terminator) > capacity {
        return Err(format!(
            "文本需要 {} 字节，现有容量为 {capacity} 字节；不支持扩容",
            bytes.len() + terminator
        ));
    }
    bytes.resize(capacity, 0);
    Ok(bytes)
}

pub struct FieldValue {
    pub display: String,
    pub edit: FieldType,
}

pub fn typed(value: impl ToString, edit: FieldType) -> FieldValue {
    FieldValue {
        display: value.to_string(),
        edit,
    }
}

pub struct Formatted<T> {
    value: T,
    display: String,
}

pub fn formatted<T>(value: T, display: impl ToString) -> Formatted<T> {
    Formatted {
        value,
        display: display.to_string(),
    }
}

impl<T: IntoFieldValue> IntoFieldValue for Formatted<T> {
    fn into_field_value(self, size: usize) -> FieldValue {
        let mut value = self.value.into_field_value(size);
        value.display = self.display;
        value
    }
}

pub trait IntoFieldValue {
    fn into_field_value(self, size: usize) -> FieldValue;
}

impl IntoFieldValue for FieldValue {
    fn into_field_value(self, _: usize) -> FieldValue {
        self
    }
}

macro_rules! scalar_values {
    ($($ty:ty => $variant:ident),* $(,)?) => {$(
        impl IntoFieldValue for $ty {
            fn into_field_value(self, size: usize) -> FieldValue {
                let scalar = ScalarType::$variant;
                let format = if size == scalar.size() {
                    FieldType::Scalar(scalar)
                } else {
                    FieldType::ReadOnly
                };
                typed(self, format)
            }
        }

        impl IntoFieldValue for &$ty {
            fn into_field_value(self, size: usize) -> FieldValue {
                (*self).into_field_value(size)
            }
        }

        impl<const N: usize> IntoFieldValue for [$ty; N] {
            fn into_field_value(self, _: usize) -> FieldValue {
                typed(format!("{self:?}"), FieldType::Array(ScalarType::$variant))
            }
        }

        impl IntoFieldValue for &Vec<$ty> {
            fn into_field_value(self, _: usize) -> FieldValue {
                typed(format!("{self:?}"), FieldType::Array(ScalarType::$variant))
            }
        }
    )*};
}
scalar_values! { u8 => U8, u16 => U16, u32 => U32, u64 => U64, i8 => I8, i16 => I16, i32 => I32, i64 => I64, f32 => F32, f64 => F64 }

impl IntoFieldValue for usize {
    fn into_field_value(self, size: usize) -> FieldValue {
        typed(
            self,
            match size {
                1 => FieldType::Scalar(ScalarType::U8),
                2 => FieldType::Scalar(ScalarType::U16),
                4 => FieldType::Scalar(ScalarType::U32),
                8 => FieldType::Scalar(ScalarType::U64),
                _ => FieldType::ReadOnly,
            },
        )
    }
}

macro_rules! readonly_values {
    ($($ty:ty),* $(,)?) => {$(
        impl IntoFieldValue for $ty {
            fn into_field_value(self, _: usize) -> FieldValue {
                typed(self, FieldType::ReadOnly)
            }
        }
    )*};
}
readonly_values! { String, &str, &String, bool }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_byte_input_rejects_signs_and_incomplete_hex_digits() {
        for invalid in ["+F", "-1", "0?", "F", "GG"] {
            assert!(FieldType::Bytes.encode(&[0], invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            FieldType::Bytes.encode(&[0, 0], "0f A0").unwrap(),
            [0x0f, 0xa0]
        );
    }

    #[test]
    fn reader_results_supply_bindings_and_readonly_fields_keep_their_codec() {
        let buffers: Vec<Arc<[u8]>> = vec![Arc::from([0xaa, 0, 1, 0xff, 0xfe, 0xbb])];
        let reader = binary::Reader::with_base(&buffers[0][1..5], 1).with_endian(Endian::Big);
        let field = Field::from_binary("向量", 0, reader.read_at::<[i16; 2]>(0).unwrap());
        assert_eq!(field.binding.range, 1..5);
        assert_eq!(field.binding.endian, Endian::Big);
        assert_eq!(field.binding.format, FieldType::Array(ScalarType::I16));
        assert_eq!(field.read(&buffers).unwrap(), "1, -2");
        assert_eq!(
            field
                .write(&buffers, "-32768, 32767")
                .unwrap()
                .unwrap()
                .after,
            [0x80, 0, 0x7f, 0xff]
        );
        let field = field.read_only();
        assert_eq!(field.read(&buffers).unwrap(), "1, -2");
        assert!(field.write(&buffers, "2, -3").is_err());
    }

    #[test]
    fn color_and_flags_preserve_storage_order_and_integer_width() {
        let color = Binding {
            buffer: 0,
            range: 0..4,
            format: FieldType::Color { alpha: true },
            endian: Endian::Little,
        };
        assert_eq!(
            color.encode(&[1, 2, 3, 0], "255, 128, 64, 0").unwrap(),
            [255, 128, 64, 0]
        );
        assert!(color.encode(&[1, 2, 3, 0], "255, 128, 64").is_err());
        let flags = Binding {
            buffer: 0,
            range: 0..4,
            format: FieldType::Flags(ScalarType::U32),
            endian: Endian::Big,
        };
        assert_eq!(flags.decode(&[0x80, 0, 0, 1]).unwrap(), "0x80000001");
        assert_eq!(
            flags.encode(&[0; 4], "0x80000001").unwrap(),
            [0x80, 0, 0, 1]
        );
        assert_eq!(
            FieldType::Flags(ScalarType::U64)
                .encode(&[0; 8], "0xFFFFFFFFFFFFFFFF")
                .unwrap(),
            [0xff; 8]
        );
        assert!(
            FieldType::Flags(ScalarType::I16)
                .encode(&[0; 2], "-1")
                .is_err()
        );
    }

    #[test]
    fn bindings_own_storage_bounds_and_produce_only_changed_patches() {
        let buffers: Vec<Arc<[u8]>> =
            vec![Arc::from([0xaa; 4]), Arc::from([0xcc, 0x34, 0x12, 0xdd])];
        let field = Field {
            name: "引用字段".into(),
            value: "formatted description is not storage".into(),
            note: None,
            writable: true,
            binding: Binding {
                buffer: 1,
                range: 1..3,
                format: FieldType::Scalar(ScalarType::U16),
                endian: Endian::Little,
            },
        };
        assert_eq!(field.read(&buffers).unwrap(), "4660");
        assert!(field.write(&buffers, "0x1234").unwrap().is_none());
        let patch = field.write(&buffers, "65535").unwrap().unwrap();
        assert_eq!(patch.binding, field.binding);
        assert_eq!(patch.before, [0x34, 0x12]);
        assert_eq!(patch.after, [0xff, 0xff]);
        assert_eq!(&*buffers[1], &[0xcc, 0x34, 0x12, 0xdd]);
        assert!(field.write(&buffers, "65536").is_err());
        let raw = Binding {
            format: FieldType::Bytes,
            ..field.binding.clone()
        };
        assert_eq!(
            raw.write(&buffers, "AB CD").unwrap().unwrap().after,
            [0xab, 0xcd]
        );
        for binding in [
            Binding {
                buffer: 2,
                ..field.binding.clone()
            },
            Binding {
                range: 3..6,
                ..field.binding.clone()
            },
            Binding {
                range: 0..1,
                ..field.binding.clone()
            },
            Binding {
                format: FieldType::ReadOnly,
                ..field.binding.clone()
            },
        ] {
            assert!(binding.read(&buffers).is_err());
            assert!(binding.write(&buffers, "1").is_err());
        }
    }

    #[test]
    fn integer_bounds_and_radices_match_storage() {
        for (kind, min, max) in [
            (ScalarType::I8, "-128", "127"),
            (ScalarType::U16, "0", "65535"),
            (
                ScalarType::I64,
                "-9223372036854775808",
                "9223372036854775807",
            ),
            (ScalarType::U64, "0", "18446744073709551615"),
        ] {
            let field = FieldType::Scalar(kind);
            for value in [min, max] {
                let bytes = field.encode(&vec![0; kind.size()], value).unwrap();
                assert_eq!(field.decode(&bytes).unwrap(), value);
            }
        }
        assert!(
            FieldType::Scalar(ScalarType::U8)
                .encode(&[0], "256")
                .is_err()
        );
        assert!(
            FieldType::Scalar(ScalarType::I8)
                .encode(&[0], "-129")
                .is_err()
        );
        assert!(
            FieldType::Scalar(ScalarType::U64)
                .encode(&[0; 8], "-1")
                .is_err()
        );
        assert_eq!(
            FieldType::Scalar(ScalarType::U16)
                .encode(&[0, 0], "0xAB_CD")
                .unwrap(),
            [0xcd, 0xab]
        );
        assert_eq!(
            Binding {
                buffer: 0,
                range: 0..4,
                format: FieldType::Scalar(ScalarType::U32),
                endian: Endian::Big
            }
            .encode(&[0; 4], "0x01020304")
            .unwrap(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn array_edit_preserves_unmodified_float_payload() {
        let mut original = 0x7fc0_1234_u32.to_le_bytes().to_vec();
        original.extend_from_slice(&1_f32.to_le_bytes());
        let output = FieldType::Array(ScalarType::F32)
            .encode(&original, "NaN, 2")
            .unwrap();
        assert_eq!(&output[..4], &original[..4]);
        assert_eq!(&output[4..], &2_f32.to_le_bytes());
        assert!(
            FieldType::Array(ScalarType::F32)
                .encode(&original, "1")
                .is_err()
        );
        assert!(
            FieldType::Scalar(ScalarType::F32)
                .encode(&[0; 4], "1e100")
                .is_err()
        );
        let original = [
            0x7ff8_0000_0000_1234_u64,
            (-0_f64).to_bits(),
            1_f64.to_bits(),
        ]
        .into_iter()
        .flat_map(u64::to_be_bytes)
        .collect::<Vec<_>>();
        let field = Binding {
            buffer: 0,
            range: 0..original.len(),
            format: FieldType::Array(ScalarType::F64),
            endian: Endian::Big,
        };
        let output = field.encode(&original, "[NaN; -0; 2]").unwrap();
        assert_eq!(&output[..16], &original[..16]);
        assert_eq!(&output[16..], &2_f64.to_be_bytes());
        assert!(field.encode(&original, "1, 2, 3, 4").is_err());
    }

    #[test]
    fn text_edit_respects_encoding_terminator_and_capacity() {
        for encoding in [
            TextEncoding::ShiftJis,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
            TextEncoding::Utf8,
        ] {
            let field = FieldType::Text {
                encoding,
                terminated: true,
            };
            let original = vec![0; 16];
            let output = field.encode(&original, "日本語").unwrap();
            assert_eq!(output.len(), original.len());
            assert_eq!(field.decode(&output).unwrap(), "日本語");
            assert!(field.encode(&original, "文字数が多すぎるテキスト").is_err());
            assert!(field.encode(&original, "a\0b").is_err());
        }
        assert!(
            FieldType::Text {
                encoding: TextEncoding::ShiftJis,
                terminated: true
            }
            .encode(&[0; 16], "🐈")
            .is_err()
        );
        let original = [b'a', 0, 0x91, 0xfe];
        assert_eq!(
            FieldType::Text {
                encoding: TextEncoding::Utf8,
                terminated: true
            }
            .encode(&original, "a")
            .unwrap(),
            original
        );
        for (encoding, bytes) in [
            (TextEncoding::Utf16Le, [0x3d, 0xd8, 0x08, 0xdc]),
            (TextEncoding::Utf16Be, [0xd8, 0x3d, 0xdc, 0x08]),
        ] {
            let field = FieldType::Text {
                encoding,
                terminated: false,
            };
            assert_eq!(field.decode(&bytes).unwrap(), "🐈");
            assert_eq!(field.encode(&bytes, "🐈").unwrap(), bytes);
            assert!(field.decode(&bytes[..2]).is_err());
            assert!(field.decode(&bytes[..3]).is_err());
        }
    }
}
