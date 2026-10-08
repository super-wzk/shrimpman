//! 工作线程合并后台请求，通过请求号让界面丢弃过期结果。
//! 文件读取、解压、编辑和导出都不占用游戏渲染线程。

mod export;
mod files;

use export::{Export, export_bytes};
use files::{pack_bytes, read_attack_directory, read_bytes, read_document};

use crate::{
    action::NodeAction,
    catalog::Catalog,
    edit,
    field::Patch,
    inspect::{self, Document},
};
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
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

#[derive(Default)]
struct Pending {
    scan: bool,
    load: Option<(u64, PathBuf, PathBuf)>,
    expand: Option<(u64, Arc<Document>, usize)>,
    export: Option<Export>,
    edit: Option<Edit>,
    pack: Option<Pack>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        !self.scan
            && self.load.is_none()
            && self.expand.is_none()
            && self.export.is_none()
            && self.edit.is_none()
            && self.pack.is_none()
    }
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    updates: Mutex<Updates>,
    wake: Condvar,
    stopped: AtomicBool,
}

impl Shared {
    fn pending(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn updates(&self) -> MutexGuard<'_, Updates> {
        self.updates.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(root: PathBuf, exports: PathBuf) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        shared.pending().scan = true;
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("mhf-workbench-io".into())
            .spawn(move || {
                let mut attack_directories = HashMap::new();
                loop {
                    let work = {
                        let mut pending = state.pending();
                        while pending.is_empty() && !state.stopped.load(Ordering::Acquire) {
                            pending = state
                                .wake
                                .wait(pending)
                                .unwrap_or_else(PoisonError::into_inner);
                        }
                        // 关闭时可以取消读取，但必须排空已接受的打包和导出，避免丢失保存请求。
                        if state.stopped.load(Ordering::Acquire)
                            && pending.pack.is_none()
                            && pending.export.is_none()
                        {
                            break;
                        }
                        std::mem::take(&mut *pending)
                    };
                    if work.scan && !state.stopped.load(Ordering::Acquire) {
                        attack_directories.clear();
                        let catalog = Catalog::scan(&root, &state.stopped)
                            .map(Arc::new)
                            .map_err(|error| error.to_string());
                        state.updates().catalog = Some(catalog);
                    }
                    if let Some((request, path, source_root)) = work.load
                        && !state.stopped.load(Ordering::Acquire)
                    {
                        let document = read_document(&path).map(|mut document| {
                            let sdt_path = source_root.join("mhfsdt.bin");
                            if path == sdt_path {
                                let directory = Arc::new(document.parsed_attack_directory());
                                attack_directories.insert(sdt_path, directory.clone());
                                document.attack_directory = Some(directory);
                            } else if document
                                .nodes
                                .iter()
                                .any(|node| node.kind == inspect::Kind::Dat)
                            {
                                let directory = attack_directories
                                    .entry(sdt_path.clone())
                                    .or_insert_with(|| Arc::new(read_attack_directory(&sdt_path)));
                                document.attack_directory = Some(directory.clone());
                            }
                            Arc::new(document)
                        });
                        state.updates().loaded = Some(Loaded {
                            request,
                            path,
                            document,
                        });
                    }
                    if let Some((request, document, node)) = work.expand
                        && !state.stopped.load(Ordering::Acquire)
                    {
                        let document = inspect::expand(&document, node).map(Arc::new);
                        state.updates().expanded = Some(Expanded { request, document });
                    }
                    if let Some(export) = work.export {
                        let result =
                            export_bytes(&exports, &export).map_err(|error| error.to_string());
                        state.updates().exported = Some(result);
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
                        state.updates().edited = Some(Expanded {
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
                        state.updates().packed = Some(Packed {
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
        self.shared.pending().scan = true;
        self.shared.wake.notify_one();
    }

    pub fn load(&self, request: u64, path: PathBuf, source_root: PathBuf) {
        // 新选择覆盖尚未执行的读取；已经开始的旧读取由界面按请求号丢弃。
        let mut pending = self.shared.pending();
        pending.load = Some((request, path, source_root));
        pending.expand = None;
        drop(pending);
        self.shared.wake.notify_one();
    }

    pub fn expand(&self, request: u64, document: Arc<Document>, node: usize) {
        self.shared.pending().expand = Some((request, document, node));
        self.shared.wake.notify_one();
    }

    pub fn export(&self, document: &Document, node: usize) -> Result<(), String> {
        let export = Export::from_node(document, node)?;
        let mut pending = self.shared.pending();
        if pending.export.is_some() {
            return Err("请等待当前导出完成".into());
        }
        pending.export = Some(export);
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn edit(&self, request: u64, document: Arc<Document>, patches: Vec<Patch>) {
        self.queue_edit(request, document, EditOperation::Fields(patches));
    }

    pub fn replace(&self, request: u64, document: Arc<Document>, node: usize, path: PathBuf) {
        self.queue_edit(request, document, EditOperation::Replace { node, path });
    }

    pub fn node_action(
        &self,
        request: u64,
        document: Arc<Document>,
        node: usize,
        action: NodeAction,
    ) {
        self.queue_edit(
            request,
            document,
            EditOperation::NodeAction { node, action },
        );
    }

    fn queue_edit(&self, request: u64, document: Arc<Document>, operation: EditOperation) {
        let mut pending = self.shared.pending();
        // 编辑会改变节点布局，旧展开请求不能在编辑完成后继续发布。
        pending.expand = None;
        pending.edit = Some(Edit {
            request,
            document,
            operation,
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
        self.shared.pending().pack = Some(Pack {
            source,
            source_root,
            root,
            document,
            patches,
        });
        self.shared.wake.notify_one();
    }

    pub fn updates(&self) -> Updates {
        std::mem::take(&mut *self.shared.updates())
    }

    pub fn stop(&mut self) {
        {
            // 停止标志与条件变量使用同一把锁，避免线程准备休眠时错过退出通知。
            let _pending = self.shared.pending();
            self.shared.stopped.store(true, Ordering::Release);
        }
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
