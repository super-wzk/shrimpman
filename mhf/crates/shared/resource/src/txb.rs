//! TXB texture bundles: a count followed by (offset, size) directory entries.
//! The same directory layout is shared with ordinary MHF archives. Texture
//! formats are identified from each entry's bytes, never from the .txb suffix.

use crate::{
    Error, Result,
    container::{DirectoryKind, Entry, SimpleArchive},
    dds::Dds,
    png::Png,
};

#[derive(Clone, Debug)]
pub enum Image<'a> {
    Png(Png<'a>),
    Dds(Dds<'a>),
    Empty,
    Unknown(&'a [u8]),
}

impl<'a> Image<'a> {
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            Self::Png(png) => Some((png.header.width, png.header.height)),
            Self::Dds(dds) => Some((dds.header.width, dds.header.height)),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        match self {
            Self::Png(png) => png.as_bytes(),
            Self::Dds(dds) => dds.as_bytes(),
            Self::Unknown(bytes) => bytes,
            Self::Empty => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Texture<'a> {
    pub entry: Entry,
    pub image: Image<'a>,
}

#[derive(Clone, Debug)]
pub struct Txb<'a> {
    pub archive: SimpleArchive<'a>,
    pub textures: Vec<Texture<'a>>,
}

impl<'a> Txb<'a> {
    /// Source must already be unwrapped with `container::open_layers` if needed.
    /// Empty and aliased entries retain their exact directory slots.
    pub fn parse(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let archive = SimpleArchive::parse(source, max_entries)?;
        if archive.kind != DirectoryKind::OffsetSize {
            return Err(Error::new(0, "expected TXB offset/size directory"));
        }
        let mut textures = Vec::new();
        for &entry in &archive.entries {
            let bytes = entry.payload(source)?;
            let image = if bytes.starts_with(&crate::png::MAGIC) {
                Image::Png(
                    Png::parse(bytes)
                        .map_err(|e| Error::new(entry.offset as usize + e.offset, e.message))?,
                )
            } else if bytes.starts_with(&crate::dds::MAGIC) {
                Image::Dds(
                    Dds::parse(bytes)
                        .map_err(|e| Error::new(entry.offset as usize + e.offset, e.message))?,
                )
            } else if bytes.is_empty() {
                Image::Empty
            } else {
                Image::Unknown(bytes)
            };
            textures.push(Texture { entry, image });
        }
        Ok(Self { archive, textures })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.archive.source
    }

    /// Verbatim image bytes, including its original DDS/PNG headers and tails.
    pub fn extract(&self, index: usize) -> Result<&'a [u8]> {
        self.archive.payload(index)
    }
}

#[cfg(test)]
#[path = "tests/txb.rs"]
mod tests;
