//! File-relative resource identities, independent of inspection-tree layout.
//!
//! `file#internal/path` records original zero-based indices and stable schema
//! keys. Format adapters assign their meaning; encoding layers are transparent
//! unless an adapter explicitly identifies a layer. This type does not resolve
//! bytes, native pointers, or filesystem paths.

use std::{fmt, str::FromStr};

/// One original resource index or stable ASCII schema key.
/// Unknown keys remain fields; parsers do not replace them with UI node indices.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Index(u32),
    Field(String),
}

/// A game-root-relative source and an ordered path inside that resource.
///
/// Canonical text uses `/` separators, decimal indices, and one optional `#`.
/// The source's literal `#` and `%` are escaped as `%23` and `%25`. Unicode and
/// case are preserved; source construction takes unescaped text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourcePath {
    source: String,
    segments: Vec<PathSegment>,
}

/// Invalid resource-address text or parts. Validation never reads the filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePathError(&'static str);

impl fmt::Display for ResourcePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ResourcePathError {}

impl ResourcePath {
    /// Construct a file identity from an unescaped game-relative source.
    /// Windows separators become `/`; absolute paths, empty components, `.`
    /// and `..` components, and control characters are rejected.
    pub fn new(source: impl Into<String>) -> Result<Self, ResourcePathError> {
        Self::from_parts(source, [])
    }

    /// Build an identity from original indices and schema keys supplied by a
    /// format adapter. Display names and inspection-tree ordinals are not keys.
    pub fn from_parts(
        source: impl Into<String>,
        segments: impl IntoIterator<Item = PathSegment>,
    ) -> Result<Self, ResourcePathError> {
        let source = source.into().replace('\\', "/");
        validate_source(&source)?;
        let segments: Vec<_> = segments.into_iter().collect();
        for segment in &segments {
            validate_segment(segment)?;
        }
        Ok(Self { source, segments })
    }

    /// Unescaped source with normalized separators and original case.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Validate before appending, leaving this identity unchanged on error.
    pub fn push(&mut self, segment: PathSegment) -> Result<(), ResourcePathError> {
        validate_segment(&segment)?;
        self.segments.push(segment);
        Ok(())
    }
}

fn validate_source(source: &str) -> Result<(), ResourcePathError> {
    if source.is_empty() {
        return Err(ResourcePathError("resource source is empty"));
    }
    if source.starts_with('/')
        || source.as_bytes().get(1) == Some(&b':') && source.as_bytes()[0].is_ascii_alphabetic()
    {
        return Err(ResourcePathError("resource source must be game-relative"));
    }
    if source.chars().any(char::is_control) {
        return Err(ResourcePathError(
            "resource source contains a control character",
        ));
    }
    if source
        .split('/')
        .any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(ResourcePathError(
            "resource source contains an empty, '.' or '..' component",
        ));
    }
    Ok(())
}

fn validate_segment(segment: &PathSegment) -> Result<(), ResourcePathError> {
    if let PathSegment::Field(field) = segment {
        let mut bytes = field.bytes();
        if !bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(ResourcePathError(
                "resource field must be an ASCII schema identifier",
            ));
        }
    }
    Ok(())
}

impl FromStr for ResourcePath {
    type Err = ResourcePathError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (source, internal) = text
            .split_once('#')
            .map_or((text, None), |(source, internal)| (source, Some(internal)));
        let source = decode_source(source)?;
        let mut segments = Vec::new();
        if let Some(internal) = internal {
            for segment in internal.split('/') {
                if segment.is_empty() {
                    return Err(ResourcePathError(
                        "resource internal path contains an empty segment",
                    ));
                }
                let segment = if segment.bytes().all(|byte| byte.is_ascii_digit()) {
                    PathSegment::Index(
                        segment
                            .parse()
                            .map_err(|_| ResourcePathError("resource index exceeds u32"))?,
                    )
                } else {
                    PathSegment::Field(segment.to_owned())
                };
                segments.push(segment);
            }
        }
        Self::from_parts(source, segments)
    }
}

fn decode_source(source: &str) -> Result<String, ResourcePathError> {
    let mut decoded = Vec::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut position = 0;
    while let Some(&byte) = bytes.get(position) {
        if byte == b'%' {
            let digits = bytes
                .get(position + 1..position + 3)
                .ok_or(ResourcePathError(
                    "resource source contains an incomplete percent escape",
                ))?;
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            let high = hex(digits[0]).ok_or(ResourcePathError("invalid source percent escape"))?;
            let low = hex(digits[1]).ok_or(ResourcePathError("invalid source percent escape"))?;
            decoded.push(high * 16 + low);
            position += 3;
        } else {
            decoded.push(byte);
            position += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| ResourcePathError("source percent escapes must form valid UTF-8"))
}

impl fmt::Display for ResourcePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.source.chars() {
            match character {
                '#' => f.write_str("%23")?,
                '%' => f.write_str("%25")?,
                character => write!(f, "{character}")?,
            }
        }
        for (index, segment) in self.segments.iter().enumerate() {
            f.write_str(if index == 0 { "#" } else { "/" })?;
            match segment {
                PathSegment::Index(index) => write!(f, "{index}")?,
                PathSegment::Field(field) => f.write_str(field)?,
            }
        }
        Ok(())
    }
}
