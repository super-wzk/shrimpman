//! 资源读取与重定向文件保存；所有调用均在工作线程执行。

use crate::inspect::{self, Document};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

/// 在目标目录完整写入临时文件后再替换重定向文件，避免发布不完整资源。
pub(super) fn pack_bytes(
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
    // 校验真实路径，避免符号链接把输出导向原资源目录或替换目录之外。
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

pub(super) fn read_document(path: &Path) -> Result<Document, String> {
    let bytes = read_bytes(path)?;
    Ok(inspect::inspect(path, bytes.into()))
}

pub(super) fn read_attack_directory(
    path: &Path,
) -> Result<mhf_resource::action_definition::AttackDirectory, String> {
    let bytes = read_bytes(path)?;
    let decoded = mhf_resource::container::open_layers(&bytes, usize::MAX, 8)
        .map_err(|error| error.to_string())?;
    let file =
        mhf_resource::sdt::Sdt::parse(decoded.payload()).map_err(|error| error.to_string())?;
    mhf_resource::action_definition::AttackDirectory::from_sdt("mhfsdt.bin", &file)
        .map_err(|error| error.to_string())
}

pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
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
    // 文件可能在打开后增长或缩短，元数据只用于预留容量；按实际读取长度逐块扩容，
    // 每次分配都保留可恢复的错误路径，避免大资源让整个游戏进程直接退出。
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

#[cfg(test)]
mod tests;
