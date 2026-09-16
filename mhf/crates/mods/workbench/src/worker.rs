//! Disk reads, decompression and exports stay off the game's render thread.

use crate::{
    action::NodeAction,
    catalog::Catalog,
    edit,
    field::Patch,
    inspect::{self, Document, Kind},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

pub(crate) struct Loaded {
    pub request: u64,
    pub path: PathBuf,
    pub document: Result<Arc<Document>, String>,
}

pub(crate) struct Expanded {
    pub request: u64,
    pub document: Result<Arc<Document>, String>,
}

#[derive(Default)]
pub(crate) struct Updates {
    pub catalog: Option<Result<Arc<Catalog>, String>>,
    pub loaded: Option<Loaded>,
    pub expanded: Option<Expanded>,
    pub edited: Option<Expanded>,
    pub packed: Option<Packed>,
    pub exported: Option<Result<PathBuf, String>>,
}

pub(crate) struct Packed {
    pub source: PathBuf,
    pub requested: Arc<Document>,
    pub result: Result<(PathBuf, Arc<Document>), String>,
}

struct Edit {
    request: u64,
    document: Arc<Document>,
    operation: EditOperation,
}

enum EditOperation {
    Fields(Vec<Patch>),
    Replace { node: usize, path: PathBuf },
    NodeAction { node: usize, action: NodeAction },
}

struct Pack {
    source: PathBuf,
    source_root: PathBuf,
    root: PathBuf,
    document: Arc<Document>,
    patches: Vec<Patch>,
}

struct Export {
    name: String,
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

impl Export {
    fn from_node(document: &Document, index: usize) -> Result<Self, String> {
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
            let extension = if node.error.is_none() {
                resource_extension(node.kind)
            } else {
                None
            };
            if let Some(extension) = extension {
                let mut path = PathBuf::from(name);
                path.set_extension(extension);
                name = path.to_string_lossy().into_owned();
            } else {
                // This byte range can still contain offsets into the parent resource.
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

/// Only standalone resource ranges receive format extensions. A generic offset
/// directory has no reliable file suffix without its original filename.
fn resource_extension(kind: Kind) -> Option<&'static str> {
    match kind {
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
    // Keep the suffix and collision number intact at the Windows filename limit.
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

#[derive(Default)]
struct Pending {
    scan: bool,
    load: Option<(u64, PathBuf)>,
    expand: Option<(u64, Arc<Document>, usize)>,
    export: Option<Export>,
    edit: Option<Edit>,
    pack: Option<Pack>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    updates: Mutex<Updates>,
    wake: Condvar,
    stopped: AtomicBool,
}

pub(crate) struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(root: PathBuf, exports: PathBuf) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scan = true;
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("mhf-workbench-io".into())
            .spawn(move || {
                loop {
                    let work = {
                        let mut pending =
                            state.pending.lock().unwrap_or_else(PoisonError::into_inner);
                        while !pending.scan
                            && pending.load.is_none()
                            && pending.export.is_none()
                            && pending.expand.is_none()
                            && pending.edit.is_none()
                            && pending.pack.is_none()
                            && !state.stopped.load(Ordering::Acquire)
                        {
                            pending = state
                                .wake
                                .wait(pending)
                                .unwrap_or_else(PoisonError::into_inner);
                        }
                        if state.stopped.load(Ordering::Acquire)
                            && pending.pack.is_none()
                            && pending.export.is_none()
                        {
                            break;
                        }
                        std::mem::take(&mut *pending)
                    };
                    if work.scan && !state.stopped.load(Ordering::Acquire) {
                        let catalog = Catalog::scan(&root, &state.stopped)
                            .map(Arc::new)
                            .map_err(|error| error.to_string());
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .catalog = Some(catalog);
                    }
                    if let Some((request, path)) = work.load
                        && !state.stopped.load(Ordering::Acquire)
                    {
                        let document = read_document(&path).map(Arc::new);
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .loaded = Some(Loaded {
                            request,
                            path,
                            document,
                        });
                    }
                    if let Some((request, document, node)) = work.expand
                        && !state.stopped.load(Ordering::Acquire)
                    {
                        let document = inspect::expand(&document, node).map(Arc::new);
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .expanded = Some(Expanded { request, document });
                    }
                    if let Some(export) = work.export {
                        let result =
                            export_bytes(&exports, &export).map_err(|error| error.to_string());
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .exported = Some(result);
                    }
                    if let Some(edit) = work.edit
                        && !state.stopped.load(Ordering::Acquire)
                    {
                        let document = match edit.operation {
                            EditOperation::Fields(patches) => {
                                edit::apply_many(&edit.document, &patches)
                            }
                            EditOperation::Replace { node, path } => read_bytes(&path)
                                .and_then(|bytes| edit::replace(&edit.document, node, &bytes)),
                            EditOperation::NodeAction { node, action } => {
                                edit::apply_node_action(&edit.document, node, action)
                            }
                        }
                        .map(Arc::new);
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .edited = Some(Expanded {
                            request: edit.request,
                            document,
                        });
                    }
                    if let Some(pack) = work.pack {
                        let result = edit::prepare_pack(&pack.document, &pack.patches).and_then(
                            |document| {
                                let path = pack_bytes(
                                    &pack.source_root,
                                    &pack.root,
                                    &pack.source,
                                    &document.buffers[0],
                                )
                                .map_err(|error| error.to_string())?;
                                Ok((path, Arc::new(document)))
                            },
                        );
                        state
                            .updates
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .packed = Some(Packed {
                            source: pack.source,
                            requested: pack.document,
                            result,
                        });
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn scan(&self) {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scan = true;
        self.shared.wake.notify_one();
    }

    pub fn load(&self, request: u64, path: PathBuf) {
        // A new selection replaces pending work. The UI discards an older read
        // already in flight by comparing its request number.
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        pending.load = Some((request, path));
        pending.expand = None;
        drop(pending);
        self.shared.wake.notify_one();
    }

    pub fn expand(&self, request: u64, document: Arc<Document>, node: usize) {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expand = Some((request, document, node));
        self.shared.wake.notify_one();
    }

    pub fn export(&self, document: &Document, node: usize) -> Result<(), String> {
        let export = Export::from_node(document, node)?;
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pending.export.is_some() {
            return Err("请等待当前导出完成".into());
        }
        pending.export = Some(export);
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn edit(&self, request: u64, document: Arc<Document>, patches: Vec<Patch>) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        pending.expand = None;
        pending.edit = Some(Edit {
            request,
            document,
            operation: EditOperation::Fields(patches),
        });
        self.shared.wake.notify_one();
    }

    pub fn replace(&self, request: u64, document: Arc<Document>, node: usize, path: PathBuf) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        pending.expand = None;
        pending.edit = Some(Edit {
            request,
            document,
            operation: EditOperation::Replace { node, path },
        });
        self.shared.wake.notify_one();
    }

    pub fn node_action(
        &self,
        request: u64,
        document: Arc<Document>,
        node: usize,
        action: NodeAction,
    ) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        pending.expand = None;
        pending.edit = Some(Edit {
            request,
            document,
            operation: EditOperation::NodeAction { node, action },
        });
        self.shared.wake.notify_one();
    }

    pub fn pack(
        &self,
        source: PathBuf,
        source_root: PathBuf,
        root: PathBuf,
        document: Arc<Document>,
        patches: Vec<Patch>,
    ) {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pack = Some(Pack {
            source,
            source_root,
            root,
            document,
            patches,
        });
        self.shared.wake.notify_one();
    }

    pub fn updates(&self) -> Updates {
        std::mem::take(
            &mut *self
                .shared
                .updates
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    pub fn stop(&mut self) {
        {
            // Serialize the predicate change with Condvar::wait to avoid a
            // lost shutdown notification while the worker enters its wait.
            let _pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.shared.stopped.store(true, Ordering::Release);
        }
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Replace only the corresponding override, after a complete write beside it.
fn pack_bytes(
    data_root: &Path,
    output_root: &Path,
    source: &Path,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    use std::{path::Component, sync::atomic::AtomicU64};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    let relative = source
        .strip_prefix(data_root)
        .map_err(|_| invalid("资源不在工作台数据目录中"))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("资源相对路径无效"));
    }
    let target = output_root.join(relative);
    let parent = target.parent().ok_or_else(|| invalid("打包目标缺少目录"))?;
    fs::create_dir_all(parent)?;
    let data_root = fs::canonicalize(data_root)?;
    let canonical_parent = fs::canonicalize(parent)?;
    let canonical_output = fs::canonicalize(output_root)?;
    if canonical_parent.starts_with(&data_root) || !canonical_parent.starts_with(&canonical_output)
    {
        return Err(invalid("打包目录不能指向原资源目录或跳出替换目录"));
    }
    if fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(invalid("打包目标不能是符号链接"));
    }
    let (temporary, mut file) = loop {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".workbench-{}-{sequence}.tmp", std::process::id()));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(target)
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_document(path: &Path) -> Result<Document, String> {
    let bytes = read_bytes(path)?;
    Ok(inspect::inspect(&path.to_string_lossy(), bytes.into()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    read_resource(file, length).map_err(|error| error.to_string())
}

fn file_length(length: u64) -> io::Result<usize> {
    usize::try_from(length)
        .ok()
        .filter(|&length| length <= isize::MAX as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "资源长度超出当前进程可寻址范围"))
}

fn read_resource(mut reader: impl Read, length: u64) -> io::Result<Vec<u8>> {
    let length = file_length(length)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    // The chunk only sizes I/O. Metadata is a capacity hint: the file can grow
    // or shrink while open, and every additional allocation remains fallible.
    let mut chunk = [0; 64 * 1024];
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if bytes
            .len()
            .checked_add(count)
            .is_none_or(|length| length > isize::MAX as usize)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "资源读取长度超出当前进程可寻址范围",
            ));
        }
        bytes
            .try_reserve(count)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

fn export_bytes(directory: &Path, export: &Export) -> io::Result<PathBuf> {
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
mod tests {
    use super::*;
    use crate::inspect::Node;

    fn export_document() -> Document {
        let node = |name: &str, kind, buffer, range| Node {
            name: name.into(),
            kind,
            buffer,
            range,
            fields: Vec::new(),
            metadata: Default::default(),
            children: Vec::new(),
            action: None,
            deferred: false,
            error: None,
        };
        Document {
            root: 0,
            buffers: vec![Arc::from([1, 2, 3, 4]), Arc::from([5, 6, 7, 8, 9, 10])],
            nodes: vec![
                node("Z:\\游戏\\dat\\em001.hd.pac", Kind::Ecd, 0, 0..4),
                node("解码结果.old", Kind::Fmod, 1, 0..6),
                node("图像.old", Kind::Png, 1, 1..5),
                node("image.png", Kind::Block, 1, 2..4),
                node("motion.mot", Kind::Track, 1, 1..3),
                node("data.dds", Kind::Unknown, 1, 0..6),
            ],
        }
    }

    #[test]
    fn export_names_distinguish_original_files_resources_and_fragments() {
        let document = export_document();
        assert_eq!(
            Export::from_node(&document, 0).unwrap().name,
            "em001.hd.pac"
        );
        assert_eq!(
            Export::from_node(&document, 1).unwrap().name,
            "解码结果.fmod"
        );
        assert_eq!(Export::from_node(&document, 2).unwrap().name, "图像.png");
        for (index, range) in [(3, 2..4), (4, 1..3), (5, 0..6)] {
            let export = Export::from_node(&document, index).unwrap();
            assert!(
                export
                    .name
                    .ends_with(&format!("-b1-0x{:08X}.bin", range.start))
            );
            assert_eq!(&export.bytes[export.range], &document.buffers[1][range]);
        }
        let mut invalid = document;
        invalid.nodes[2].error = Some("truncated resource".into());
        assert!(
            Export::from_node(&invalid, 2)
                .unwrap()
                .name
                .ends_with(".bin")
        );
        invalid.nodes[2].range = 0..99;
        assert!(Export::from_node(&invalid, 2).is_err());
    }

    #[test]
    fn export_filenames_keep_unicode_and_extensions_with_safe_collision_names() {
        assert_eq!(
            safe_filename("Z:\\游戏\\怪物 001.hd.dds"),
            "怪物 001.hd.dds"
        );
        assert_eq!(safe_filename("../../raw:node?.bin"), "raw_node_.bin");
        for name in ["CON", "NUL.dds", "com1.png", "LPT².bin", "CONOUT$"] {
            assert!(safe_filename(name).starts_with('_'));
        }
        assert_eq!(safe_filename(".."), "resource");
        assert_eq!(numbered_filename("怪物 001.hd.dds", 0), "怪物 001.hd.dds");
        assert_eq!(
            numbered_filename("怪物 001.hd.dds", 1),
            "怪物 001.hd-001.dds"
        );
        let long = format!("{}.dds", "😀".repeat(150));
        for index in [999, 1000, usize::MAX] {
            let bounded = numbered_filename(&long, index);
            assert!(bounded.encode_utf16().count() <= 255);
            assert!(bounded.ends_with(&format!("-{index:03}.dds")));
        }
    }

    #[test]
    fn resource_lengths_follow_address_space_instead_of_a_fixed_file_limit() {
        let length = 256 * 1024 * 1024 + 1;
        assert_eq!(file_length(length).unwrap(), length as usize);
        assert_eq!(file_length(isize::MAX as u64).unwrap(), isize::MAX as usize);
        assert!(file_length(isize::MAX as u64 + 1).is_err());
        assert!(file_length(u64::MAX).is_err());
    }

    #[test]
    fn resource_reads_use_actual_bytes_when_metadata_length_changes() {
        let source: Vec<_> = (0..131_079).map(|index| index as u8).collect();
        for length in [0, 17, source.len() as u64, source.len() as u64 + 100] {
            assert_eq!(
                read_resource(io::Cursor::new(&source), length).unwrap(),
                source
            );
        }
    }

    #[test]
    fn exports_preserve_selected_bytes_and_never_overwrite_existing_files() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-export-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let export = Export {
            name: "../raw:node.bin".into(),
            bytes: Arc::from([0, 0xff, 0x80, 4]),
            range: 1..3,
        };
        let first = export_bytes(&directory, &export).unwrap();
        let second = export_bytes(&directory, &export).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.file_name().unwrap(), "raw_node.bin");
        assert_eq!(second.file_name().unwrap(), "raw_node-001.bin");
        assert_eq!(first.parent(), Some(directory.as_path()));
        assert_eq!(fs::read(&first).unwrap(), [0xff, 0x80]);
        assert_eq!(fs::read(&second).unwrap(), [0xff, 0x80]);
        let unicode = export_bytes(
            &directory,
            &Export {
                name: "怪物 001.dds".into(),
                bytes: export.bytes.clone(),
                range: export.range.clone(),
            },
        )
        .unwrap();
        assert_eq!(unicode.file_name().unwrap(), "怪物 001.dds");
        assert_eq!(fs::read(unicode).unwrap(), [0xff, 0x80]);
        assert!(
            export_bytes(
                &directory,
                &Export {
                    range: 0..5,
                    ..export
                }
            )
            .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn packing_replaces_the_corresponding_override_and_preserves_the_source() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-pack-{}", std::process::id()));
        let data = directory.join("dat");
        let output = directory.join("dat-redirect");
        fs::create_dir_all(data.join("nested")).unwrap();
        let source = data.join("nested/mhfdat.bin");
        fs::write(&source, [1, 2, 3]).unwrap();
        let target = pack_bytes(&data, &output, &source, &[4, 5, 6]).unwrap();
        assert_eq!(target, output.join("nested/mhfdat.bin"));
        pack_bytes(&data, &output, &source, &[7, 8]).unwrap();
        assert_eq!(fs::read(&target).unwrap(), [7, 8]);
        assert_eq!(fs::read(&source).unwrap(), [1, 2, 3]);
        assert!(pack_bytes(&data, &data, &source, &[9]).is_err());
        assert!(pack_bytes(&data, &output, &data.join("../outside.bin"), &[9]).is_err());
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stopping_drains_an_accepted_save_including_pending_field_input() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-save-stop-{}", std::process::id()));
        let data = directory.join("dat");
        let output = directory.join("redirect");
        fs::create_dir_all(&data).unwrap();
        let source = data.join("value.bin");
        fs::write(&source, [1, 2]).unwrap();
        let document = Arc::new(read_document(&source).unwrap());
        let binding = crate::field::Binding {
            buffer: 0,
            range: 0..1,
            format: crate::field::FieldType::Scalar(crate::field::ScalarType::U8),
            endian: mhf_resource::binary::Endian::Little,
        };
        let patch = binding.write(&document.buffers, "9").unwrap().unwrap();
        let mut worker = Worker::start(data.clone(), directory.join("exports")).unwrap();
        worker.pack(source.clone(), data, output.clone(), document, vec![patch]);
        worker.stop();
        assert_eq!(fs::read(output.join("value.bin")).unwrap(), [9, 2]);
        assert_eq!(fs::read(source).unwrap(), [1, 2]);
        assert!(worker.updates().packed.unwrap().result.is_ok());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn packing_name_repairs_return_the_written_document_and_keep_raw_exports_verbatim() {
        use crate::session::Session;
        use mhf_resource::crypto::{Ecd, Exf};

        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-name-repair-{}", std::process::id()));
        let data = directory.join("dat");
        let output = directory.join("redirect");
        fs::create_dir_all(&data).unwrap();
        let ecd = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
            .unwrap()
            .encode(b"payload", Some(b"donor.bin"))
            .unwrap();
        let exf = Exf::parse(b"exf\x1a\x04\0\0\0\0\0\0\0\x12\x34\x56\x78")
            .unwrap()
            .encode(b"audio", Some(b"donor.mus"))
            .unwrap();
        for (name, bytes) in [("target.bin", ecd), ("target.mus", exf)] {
            let source = data.join(name);
            fs::write(&source, &bytes).unwrap();
            let document = Arc::new(read_document(&source).unwrap());
            let raw = Export::from_node(&document, document.root).unwrap();
            assert_eq!(&raw.bytes[raw.range], bytes);
            let mut session = Session::new(document.clone());
            let mut worker = Worker::start(data.clone(), directory.join("exports")).unwrap();
            worker.pack(
                source.clone(),
                data.clone(),
                output.clone(),
                document.clone(),
                Vec::new(),
            );
            worker.stop();
            let packed = worker.updates().packed.unwrap();
            let (target, saved) = packed.result.unwrap();
            assert!(Arc::ptr_eq(&packed.requested, &document));
            let written = fs::read(target).unwrap();
            assert_eq!(written.as_slice(), saved.buffers[0].as_ref());
            assert_ne!(written, bytes);
            assert_eq!(&written[..6], &bytes[..6]);
            assert_eq!(&written[8..], &bytes[8..]);
            assert_eq!(fs::read(source).unwrap(), bytes);
            match saved.nodes[saved.root].kind {
                Kind::Ecd => Ecd::parse(&written)
                    .unwrap()
                    .validate_filename(name.as_bytes())
                    .unwrap(),
                Kind::Exf => Exf::parse(&written)
                    .unwrap()
                    .validate_filename(name.as_bytes())
                    .unwrap(),
                _ => panic!("packing must preserve the wrapper kind"),
            }
            assert!(session.saved(&packed.requested, &saved));
            assert!(!session.dirty());
            let raw = Export::from_node(&session.document, session.document.root).unwrap();
            assert_eq!(&raw.bytes[raw.range], written);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn packing_rejects_symlinked_subdirectories_outside_the_override_root() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-pack-link-{}", std::process::id()));
        let data = directory.join("dat");
        let output = directory.join("redirect");
        let outside = directory.join("outside");
        for path in [&data, &output, &outside] {
            fs::create_dir_all(path).unwrap();
        }
        std::os::unix::fs::symlink(&outside, output.join("nested")).unwrap();
        assert!(pack_bytes(&data, &output, &data.join("nested/a.bin"), &[9]).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn worker_can_stop_while_scan_is_starting_or_waiting() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-stop-{}", std::process::id()));
        for _ in 0..8 {
            let mut worker = Worker::start(directory.clone(), directory.clone()).unwrap();
            worker.stop();
            worker.stop();
            assert!(worker.thread.is_none());
        }
    }
}
