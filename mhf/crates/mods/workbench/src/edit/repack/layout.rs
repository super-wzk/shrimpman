//! Layout constraints belong to a physical container, not to its children or
//! Rust's in-memory struct alignment. See resource/docs/resource-alignment.md.

use std::{num::NonZeroUsize, ops::Range};

use super::{Directory, Kind, Member};

#[derive(Clone, Copy, Debug)]
pub(super) struct Alignment {
    origin: usize,
    boundary: NonZeroUsize,
}

impl Alignment {
    const fn new(origin: usize, boundary: usize) -> Self {
        Self {
            origin,
            boundary: NonZeroUsize::new(boundary).unwrap(),
        }
    }

    fn round(self, offset: usize) -> Option<usize> {
        offset
            .checked_sub(self.origin)?
            .checked_next_multiple_of(self.boundary.get())?
            .checked_add(self.origin)
    }

    fn contains(self, offset: usize) -> bool {
        self.round(offset) == Some(offset)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Layout {
    /// Unknown gaps and trailers remain byte-for-byte intact. Tight directories
    /// also use this policy; naturally aligned offsets are not a padding rule.
    Preserve,
    Allocated {
        alignment: Alignment,
        minimum_padding: usize,
    },
    Aligned {
        alignment: Alignment,
        extra_blocks: bool,
        tail: bool,
    },
}

pub(super) struct Replacement {
    pub range: Range<usize>,
    pub padding: usize,
}

impl Layout {
    pub fn detect(directory: &Directory, source: &[u8]) -> Self {
        // Empty directories provide no evidence for selecting a layout, but
        // an already selected layout may become empty during a replacement.
        if !directory
            .members
            .iter()
            .any(|member| !member.allocation().is_empty())
        {
            return Self::Preserve;
        }
        let candidate = match directory.kind {
            Kind::Mha => Self::Allocated {
                alignment: Alignment::new(24, 512),
                minimum_padding: 24,
            },
            Kind::Momo => Self::Aligned {
                alignment: Alignment::new(0, 64),
                extra_blocks: false,
                tail: true,
            },
            Kind::Stage => Self::Aligned {
                alignment: Alignment::new(0, 16),
                extra_blocks: true,
                tail: false,
            },
            Kind::StageObjectPackage => Self::Aligned {
                alignment: Alignment::new(0, 16),
                extra_blocks: false,
                tail: false,
            },
            _ => return Self::Preserve,
        };
        // An offset/size header ends at 4 + 8*n, never a multiple of 16.
        // Thus the mandatory first gap distinguishes aligned objects from
        // tight objects even when all later sizes happen to be multiples of 16.
        if candidate.matches(directory, source) {
            candidate
        } else {
            Self::Preserve
        }
    }

    pub fn has_allocations(self) -> bool {
        matches!(self, Self::Allocated { .. })
    }

    pub fn matches(self, directory: &Directory, source: &[u8]) -> bool {
        match self {
            Self::Preserve => true,
            Self::Allocated {
                alignment,
                minimum_padding,
            } => {
                let Some(allocations) = directory.allocations() else {
                    return false;
                };
                if allocations
                    .first()
                    .is_none_or(|range| range.start != directory.header_end)
                    || allocations
                        .windows(2)
                        .any(|pair| pair[0].end != pair[1].start)
                {
                    return false;
                }
                directory.members.iter().all(|member| {
                    let Some((_, size)) = member.padded else {
                        return false;
                    };
                    if size == 0 {
                        return member.entry.size == 0;
                    }
                    alignment.contains(member.entry.offset as usize)
                        && (member.entry.size as usize)
                            .checked_add(minimum_padding)
                            .and_then(|size| {
                                size.checked_next_multiple_of(alignment.boundary.get())
                            })
                            == Some(size as usize)
                        && zeroes(source, member.payload().end..member.allocation().end)
                })
            }
            Self::Aligned {
                alignment,
                extra_blocks,
                tail,
            } => {
                let Some(allocations) = directory.allocations() else {
                    return false;
                };
                let mut end = directory.header_end;
                for range in allocations {
                    let Some(minimum) = alignment.round(end) else {
                        return false;
                    };
                    let start_matches = if extra_blocks {
                        range.start >= minimum && alignment.contains(range.start)
                    } else {
                        range.start == minimum
                    };
                    if !start_matches || !zeroes(source, end..range.start) {
                        return false;
                    }
                    end = range.end;
                }
                !tail
                    || alignment.round(end) == Some(source.len())
                        && zeroes(source, end..source.len())
            }
        }
    }

    pub fn replacement(
        self,
        directory: &Directory,
        source: &[u8],
        member: &Member,
        size: usize,
    ) -> Result<Replacement, String> {
        let mut range = member.payload();
        let padding = match self {
            Self::Preserve => 0,
            Self::Allocated {
                alignment,
                minimum_padding,
            } => {
                range = member.allocation();
                size.checked_add(minimum_padding)
                    .and_then(|size| size.checked_next_multiple_of(alignment.boundary.get()))
                    .and_then(|allocated| allocated.checked_sub(size))
                    .ok_or("成员分配长度溢出")?
            }
            Self::Aligned {
                alignment, tail, ..
            } => {
                let next = directory
                    .members
                    .iter()
                    .filter(|other| {
                        !other.allocation().is_empty() && other.entry.offset as usize > range.start
                    })
                    .map(|other| other.entry.offset as usize)
                    .min()
                    .or_else(|| tail.then_some(source.len()));
                if let Some(next) = next {
                    let old_aligned_end = alignment.round(range.end).ok_or("成员对齐位置溢出")?;
                    // Retain extra whole blocks (including Stage empty-slot
                    // reservations); only the minimum padding is recalculated.
                    let extra = next
                        .checked_sub(old_aligned_end)
                        .ok_or("原始成员填充范围无效")?;
                    let end = range.start.checked_add(size).ok_or("成员长度溢出")?;
                    let padding = alignment
                        .round(end)
                        .and_then(|aligned| aligned.checked_sub(end))
                        .and_then(|padding| padding.checked_add(extra))
                        .ok_or("成员填充长度溢出")?;
                    range.end = next;
                    padding
                } else {
                    0
                }
            }
        };
        Ok(Replacement { range, padding })
    }
}

fn zeroes(source: &[u8], range: Range<usize>) -> bool {
    source
        .get(range)
        .is_some_and(|bytes| bytes.iter().all(|&byte| byte == 0))
}
