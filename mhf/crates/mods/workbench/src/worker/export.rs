//! 节点导出、Windows 文件名处理与防覆盖写入。

use crate::inspect::{Document, Kind};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct Export {
    pub(super) name: String,
    pub(super) bytes: Arc<[u8]>,
    pub(super) range: Range<usize>,
}

impl Export {
    pub(super) fn from_node(document: &Document, index: usize) -> Result<Self, String> {
        let node = document.nodes.get(index).ok_or("导出节点无效")?;
        let bytes = document.buffers.get(node.buffer).ok_or("导出数据层无效")?;
        if bytes.get(node.range.clone()).is_none() {
            return Err("导出范围无效".into());
        }
        let original = index == document.root && node.buffer == 0 && node.range == (0..bytes.len());
        let mut name = node
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned();
        if !original {
            // 只有完整、可独立解释的资源才使用格式后缀。
            let extension = match node.kind {
                _ if node.error.is_some() => None,
                Kind::Momo => Some("momo"),
                Kind::Mha => Some("mha"),
                Kind::Txb => Some("txb"),
                Kind::Fmod => Some("fmod"),
                Kind::Fskl => Some("fskl"),
                Kind::Motion | Kind::MotionArchive => Some("mot"),
                Kind::Png => Some("png"),
                Kind::Dds => Some("dds"),
                Kind::Ogg => Some("ogg"),
                _ => None,
            };
            if let Some(extension) = extension {
                let mut path = PathBuf::from(name);
                path.set_extension(extension);
                name = path.to_string_lossy().into_owned();
            } else {
                // 内部片段可能仍引用父资源偏移，不能把它标成可独立加载的格式。
                name = format!(
                    "{name}-{}-b{}-0x{:08X}.bin",
                    node.kind.label(),
                    node.buffer,
                    node.range.start
                );
            }
        }
        Ok(Self {
            name,
            bytes: bytes.clone(),
            range: node.range.clone(),
        })
    }
}

fn safe_filename(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut name: String = name
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect();
    name.truncate(name.trim_end_matches(['.', ' ']).len());
    if name.is_empty() {
        name.push_str("resource");
    }
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || device
        .strip_prefix("COM")
        .or_else(|| device.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    {
        name.insert(0, '_');
    }
    name
}

fn numbered_filename(name: &str, index: usize) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let suffix = if index == 0 {
        String::new()
    } else {
        format!("-{index:03}")
    };
    // Windows 文件名长度按 UTF-16 码元计算，截断时保留格式后缀和防重名编号。
    let budget = 255usize.saturating_sub(extension.encode_utf16().count() + suffix.len());
    let mut units = 0;
    let stem: String = stem
        .chars()
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= budget
        })
        .collect();
    format!("{stem}{suffix}{extension}")
}

pub(super) fn export_bytes(directory: &Path, export: &Export) -> io::Result<PathBuf> {
    let bytes = export
        .bytes
        .get(export.range.clone())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid export range"))?;
    fs::create_dir_all(directory)?;
    let name = safe_filename(&export.name);
    let mut index = 0usize;
    loop {
        let path = directory.join(numbered_filename(&name, index));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                index = index.checked_add(1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "导出文件编号超出当前进程可表示范围",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
