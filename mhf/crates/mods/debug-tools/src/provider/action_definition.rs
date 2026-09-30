use mhf_resource::{
    action_definition::{AttackDirectory, Definition},
    container,
    crypto::Ecd,
    sdt,
};
use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

pub(super) struct ActionDefinition {
    pub action: super::Action,
    pub motion_style: Option<u8>,
    pub data: Result<Definition, String>,
    pub attacks: Option<Arc<Result<AttackDirectory, String>>>,
}

pub(super) struct AttackResources {
    path: PathBuf,
    directory: OnceLock<Arc<Result<AttackDirectory, String>>>,
}

impl AttackResources {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            directory: OnceLock::new(),
        }
    }

    /// Called only by InspectAction on the native task thread. The logical DAT
    /// path's read goes through the game's existing CreateFile redirection.
    pub(super) fn snapshot(
        &self,
        definition: &Definition,
    ) -> Option<Arc<Result<AttackDirectory, String>>> {
        definition
            .events
            .iter()
            .any(|event| event.attack_reference(definition.weapon).is_some())
            .then(|| self.directory.get_or_init(|| Arc::new(self.load())).clone())
    }

    fn load(&self) -> Result<AttackDirectory, String> {
        const MAX_BYTES: usize = 64 * 1024 * 1024;
        let bytes = File::open(&self.path)
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
                Ok(bytes)
            })
            .map_err(|error| format!("读取攻击资源 {} 失败：{error}", self.path.display()))?;
        if bytes.len() > MAX_BYTES {
            return Err("mhfsdt.bin 文件超过 64 MiB 读取上限".into());
        }
        if bytes.starts_with(b"ecd\x1a") {
            Ecd::parse(&bytes)
                .and_then(|file| file.validate_filename(b"mhfsdt.bin"))
                .map_err(|error| format!("校验 mhfsdt.bin 文件名失败：{error}"))?;
        }
        let opened = container::open_layers(&bytes, MAX_BYTES, 8)
            .map_err(|error| format!("解码 mhfsdt.bin 失败：{error}"))?;
        let file = sdt::Sdt::parse(opened.payload())
            .map_err(|error| format!("解析 mhfsdt.bin 原文件目录失败：{error}"))?;
        AttackDirectory::from_sdt("mhfsdt.bin", &file)
            .map_err(|error| format!("建立 mhfsdt.bin 原文件索引失败：{error}"))
    }
}

pub(super) fn read(bytes: &[u8], base: u32, action: super::Action) -> Result<Definition, String> {
    if action.group != 1 || action.weapon >= mhf_resource::action_definition::WEAPON_COUNT {
        return Err("此动作不使用 DAT[389] 武器事件目录，尚未建立静态攻击引用".into());
    }
    Definition::parse(bytes, base, action.weapon, u16::from(action.id))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
