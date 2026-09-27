//! Presentation names are independent of the stable field keys used by edits.
//! Semantic overrides are format-specific; opaque fields retain their offsets.

use super::Kind;
use std::borrow::Cow;

mod dat;
mod emd;
mod resources;

pub(super) use dat::table_label;

pub fn field_label(kind: Kind, name: &str) -> Cow<'_, str> {
    emd::label(kind, name)
        .or_else(|| resources::label(kind, name))
        .or_else(|| raw_label(name))
        .unwrap_or(Cow::Borrowed(name))
}

fn is_decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn raw_label(name: &str) -> Option<Cow<'_, str>> {
    match name {
        "opcode" => return Some(Cow::Borrowed("操作码")),
        "raw_bytes" => return Some(Cow::Borrowed("原始字节")),
        _ => {}
    }
    if let Some(index) = name
        .strip_prefix("operand_")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(Cow::Owned(format!("操作数 {index:02}")));
    }
    for (prefix, label) in [
        ("unknown_", "未知字段"),
        ("raw_", "原始字节"),
        ("word_", "原始值"),
        ("value_", "数值"),
        ("key_", "条件值"),
        ("root_", "根字段"),
        ("field_", "字段"),
        ("padding_", "填充字节"),
    ] {
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        let end = rest.bytes().take_while(u8::is_ascii_hexdigit).count();
        if end == 0 {
            continue;
        }
        let (digits, suffix) = rest.split_at(end);
        // A Chinese suffix may describe a modifier of the opaque field. Do
        // not mistake words such as raw_bytes for a hexadecimal field offset.
        if suffix.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let offset = usize::from_str_radix(digits, 16).ok()?;
        return Some(Cow::Owned(format!("{label} +0x{offset:02X}{suffix}")));
    }
    None
}
