use std::{borrow::Cow, ops::Range};

use mhf_resource::container::{DirectoryKind, Entry, MhaArchive, SimpleArchive, StageArchive};

use crate::inspect::Kind;

use super::splice;

mod layout;

struct Member {
    entry: Entry,
    offset_field: usize,
    padded: Option<(usize, u32)>,
}

impl Member {
    fn payload(&self) -> Range<usize> {
        self.entry.offset as usize..self.entry.offset as usize + self.entry.size as usize
    }

    fn allocation(&self) -> Range<usize> {
        let size = self.padded.map_or(self.entry.size, |(_, size)| size);
        self.entry.offset as usize..self.entry.offset as usize + size as usize
    }
}

struct Directory {
    kind: Kind,
    header_end: usize,
    members: Vec<Member>,
    protected: Vec<Range<usize>>,
    roots: Vec<(usize, usize)>,
}

impl Directory {
    fn parse(kind: Kind, source: &[u8]) -> Result<Self, String> {
        let mut directory = Self {
            kind,
            header_end: 0,
            members: Vec::new(),
            protected: Vec::new(),
            roots: Vec::new(),
        };
        directory.members = match kind {
            Kind::Archive
            | Kind::Momo
            | Kind::Txb
            | Kind::StageObjectPackage
            | Kind::EffectArchive => {
                let archive =
                    SimpleArchive::parse(source, source.len()).map_err(|e| e.to_string())?;
                if archive.kind == DirectoryKind::Momo {
                    directory.kind = Kind::Momo;
                }
                directory.header_end = archive.table_offset + archive.entries.len() * 8;
                directory.protected.push(0..directory.header_end);
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
                let archive =
                    StageArchive::parse(source, source.len()).map_err(|e| e.to_string())?;
                directory.header_end = 0x1c + archive.additional_count as usize * 12;
                directory.protected.push(0..directory.header_end);
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
                directory.header_end = 24;
                directory.protected.extend([
                    0..24,
                    h.entries_offset as usize
                        ..h.entries_offset as usize + archive.entries.len() * 20,
                    h.names_offset as usize..h.names_offset as usize + h.names_size as usize,
                ]);
                directory.roots.extend([
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
        Ok(directory)
    }

    /// Exact aliases share a physical allocation. Partial overlaps are not a
    /// layout profile, even when every individual directory entry is in bounds.
    fn allocations(&self) -> Option<Vec<Range<usize>>> {
        let mut ranges: Vec<_> = self
            .members
            .iter()
            .map(Member::allocation)
            .filter(|range| !range.is_empty())
            .collect();
        ranges.sort_unstable_by_key(|range| (range.start, range.end));
        ranges.dedup();
        if ranges.windows(2).any(|pair| pair[0].end > pair[1].start)
            || ranges
                .iter()
                .any(|range| self.protected.iter().any(|area| overlaps(range, area)))
        {
            None
        } else {
            Some(ranges)
        }
    }

    fn check_span(&self, target: &Member, span: &Range<usize>) -> Result<(), String> {
        let payload = target.payload();
        let allocation = target.allocation();
        if self
            .members
            .iter()
            .any(|member| member.payload() == payload && member.allocation() != allocation)
        {
            return Err("资源别名的分配范围不一致，不能重定位".into());
        }
        // Also validate declared allocation ownership for opaque layouts.
        // Falling back to preserving bytes must not legitimize a padded_size
        // that runs into another member or the directory/name table.
        let occupied = span.start.min(allocation.start)..span.end.max(allocation.end);
        for protected in &self.protected {
            if overlaps(&occupied, protected) {
                return Err("资源或填充与目录或名称区重叠，不能重定位".into());
            }
        }
        for member in &self.members {
            if member.payload() != payload && overlaps(&occupied, &member.allocation()) {
                return Err("目录成员或填充存在部分重叠，不能安全重定位".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn replace(
    kind: Kind,
    source: &[u8],
    range: Range<usize>,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    replace_many(kind, source, &[(range, payload)])
}

/// Choose a layout from this container, once per batch. Apply changes from the
/// end so earlier source ranges remain valid. Each output directory is checked
/// and reused for the next splice. Nested containers choose their own layout.
pub(super) fn replace_many(
    kind: Kind,
    source: &[u8],
    replacements: &[(Range<usize>, &[u8])],
) -> Result<Vec<u8>, String> {
    let mut directory = Directory::parse(kind, source)?;
    let layout = layout::Layout::detect(&directory, source);
    let mut replacements: Vec<_> = replacements.iter().collect();
    replacements.sort_unstable_by_key(|(range, _)| (range.start, range.end));
    if replacements
        .windows(2)
        .any(|pair| overlaps(&pair[0].0, &pair[1].0))
    {
        return Err("批量替换范围重叠".into());
    }
    let mut output = Cow::Borrowed(source);
    for (range, payload) in replacements.into_iter().rev() {
        output = Cow::Owned(replace_one(
            &directory,
            &output,
            range.clone(),
            payload,
            layout,
        )?);
        directory = Directory::parse(directory.kind, &output)?;
        if !layout.matches(&directory, &output) {
            return Err("重打包后的资源不满足原容器的布局约束".into());
        }
    }
    Ok(output.into_owned())
}

fn replace_one(
    directory: &Directory,
    source: &[u8],
    range: Range<usize>,
    payload: &[u8],
    layout: layout::Layout,
) -> Result<Vec<u8>, String> {
    let member = directory
        .members
        .iter()
        .find(|member| member.payload() == range)
        .ok_or("替换范围不是目录中的完整成员")?;
    if range.is_empty() && member.allocation().is_empty() {
        return Err("空目录槽没有已确认的分配范围，不能插入资源".into());
    }
    let plan = layout.replacement(directory, source, member, payload.len())?;
    directory.check_span(member, &plan.range)?;
    let length = payload
        .len()
        .checked_add(plan.padding)
        .ok_or("分配长度溢出")?;
    let replacement = if plan.padding == 0 {
        Cow::Borrowed(payload)
    } else {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| "无法分配替换资源")?;
        bytes.extend_from_slice(payload);
        bytes.resize(length, 0);
        Cow::Owned(bytes)
    };
    let shift = |offset: usize| -> Result<usize, String> {
        if offset >= plan.range.end {
            offset
                .checked_sub(plan.range.len())
                .and_then(|offset| offset.checked_add(length))
                .ok_or_else(|| "目录重定位溢出".into())
        } else if offset > plan.range.start {
            Err("目录字段落在被替换资源或填充内部".into())
        } else {
            Ok(offset)
        }
    };
    let mut output = splice(source, plan.range.clone(), &replacement)?;
    for &(field, offset) in &directory.roots {
        set_u32(&mut output, shift(field)?, shift(offset)?)?;
    }
    for member in &directory.members {
        let offset_field = shift(member.offset_field)?;
        if member.payload() == range {
            set_u32(&mut output, offset_field + 4, payload.len())?;
            if let Some((field, padded)) = member.padded {
                let padded = if layout.has_allocations() {
                    length
                } else {
                    (padded as usize - member.entry.size as usize)
                        .checked_add(payload.len())
                        .ok_or("分配长度溢出")?
                };
                set_u32(&mut output, shift(field)?, padded)?;
            }
        } else if !member.allocation().is_empty() {
            // An empty MHA payload can still own a real 512-byte allocation.
            // Only completely unallocated slots retain their stale offsets.
            set_u32(
                &mut output,
                offset_field,
                shift(member.entry.offset as usize)?,
            )?;
        }
    }
    Ok(output)
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
