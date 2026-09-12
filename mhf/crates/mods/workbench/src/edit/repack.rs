use std::ops::Range;

use mhf_resource::container::{Entry, MhaArchive, SimpleArchive, StageArchive};

use crate::inspect::Kind;

use super::splice;

struct Member {
    entry: Entry,
    offset_field: usize,
    padded: Option<(usize, u32)>,
}

/// Relocate only offsets owned by a fully parsed directory. Preserve gaps,
/// trailers, names, IDs and exact aliases; reject partially overlapping data.
pub(super) fn replace(
    kind: Kind,
    source: &[u8],
    range: Range<usize>,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let mut regions = Vec::new();
    let mut roots = Vec::new();
    let members: Vec<Member> = match kind {
        Kind::Archive | Kind::Momo | Kind::Txb | Kind::StageObjectPackage | Kind::EffectArchive => {
            let archive = SimpleArchive::parse(source, source.len()).map_err(|e| e.to_string())?;
            regions.push(0..archive.table_offset + archive.entries.len() * 8);
            archive
                .entries
                .into_iter()
                .map(|entry| Member {
                    offset_field: archive.table_offset + entry.index * 8,
                    entry,
                    padded: None,
                })
                .collect()
        }
        Kind::Stage => {
            let archive = StageArchive::parse(source, source.len()).map_err(|e| e.to_string())?;
            regions.push(0..0x1c + archive.additional_count as usize * 12);
            archive
                .entries
                .into_iter()
                .map(|item| Member {
                    offset_field: if item.entry.index < 3 {
                        item.entry.index * 8
                    } else {
                        0x20 + (item.entry.index - 3) * 12
                    },
                    entry: item.entry,
                    padded: None,
                })
                .collect()
        }
        Kind::Mha => {
            let archive = MhaArchive::parse(source, source.len()).map_err(|e| e.to_string())?;
            let h = archive.header;
            regions.extend([
                0..24,
                h.entries_offset as usize..h.entries_offset as usize + archive.entries.len() * 20,
                h.names_offset as usize..h.names_offset as usize + h.names_size as usize,
            ]);
            roots.extend([
                (4, h.entries_offset as usize),
                (12, h.names_offset as usize),
            ]);
            archive
                .entries
                .into_iter()
                .map(|item| {
                    let field = h.entries_offset as usize + item.entry.index * 20;
                    Member {
                        entry: item.entry,
                        offset_field: field + 4,
                        padded: Some((field + 12, item.padded_size)),
                    }
                })
                .collect()
        }
        _ => {
            return Err(format!(
                "{} 中的编码资源长度发生变化，尚无可靠的目录重定位规则",
                kind
            ));
        }
    };
    if !members
        .iter()
        .any(|member| entry_range(member.entry) == range)
    {
        return Err("替换范围不是目录中的完整成员".into());
    }
    for protected in regions {
        if overlaps(&range, &protected) {
            return Err("资源与目录或名称区重叠，不能重定位".into());
        }
    }
    for member in &members {
        let current = entry_range(member.entry);
        if current != range && overlaps(&range, &current) {
            return Err("目录成员存在部分重叠，不能安全重定位".into());
        }
        if let Some((_, padded)) = member.padded {
            let padding = current.end..member.entry.offset as usize + padded as usize;
            if overlaps(&range, &padding) {
                return Err("替换资源与另一成员的填充区重叠".into());
            }
        }
    }
    let shift = |offset: usize| -> Result<usize, String> {
        if offset >= range.end {
            offset
                .checked_sub(range.len())
                .and_then(|offset| offset.checked_add(payload.len()))
                .ok_or_else(|| "目录重定位溢出".into())
        } else if offset > range.start {
            Err("目录字段落在被替换资源内部".into())
        } else {
            Ok(offset)
        }
    };
    let mut output = splice(source, range.clone(), payload)?;
    for (field, offset) in roots {
        set_u32(&mut output, shift(field)?, shift(offset)?)?;
    }
    for member in members {
        let offset_field = shift(member.offset_field)?;
        if entry_range(member.entry) == range {
            set_u32(&mut output, offset_field + 4, payload.len())?;
            if let Some((field, padded)) = member.padded {
                let padded = padded as usize - member.entry.size as usize + payload.len();
                set_u32(&mut output, shift(field)?, padded)?;
            }
        } else if member.entry.size != 0 {
            set_u32(
                &mut output,
                offset_field,
                shift(member.entry.offset as usize)?,
            )?;
        }
    }
    // Catch a layout mistake before it can become the edited source image.
    match kind {
        Kind::Mha => MhaArchive::parse(&output, output.len()).map(|_| ()),
        Kind::Stage => StageArchive::parse(&output, output.len()).map(|_| ()),
        _ => SimpleArchive::parse(&output, output.len()).map(|_| ()),
    }
    .map_err(|error| error.to_string())?;
    Ok(output)
}

fn entry_range(entry: Entry) -> Range<usize> {
    entry.offset as usize..entry.offset as usize + entry.size as usize
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    !left.is_empty() && !right.is_empty() && left.start < right.end && right.start < left.end
}

fn set_u32(bytes: &mut [u8], at: usize, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "目录偏移或长度超过 32 位")?;
    bytes
        .get_mut(at..at + 4)
        .ok_or("目录字段超出重打包后的资源")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
