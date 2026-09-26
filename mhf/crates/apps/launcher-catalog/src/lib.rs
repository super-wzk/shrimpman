//! Metadata for Mods compiled into the launcher, shared with its manager.

use mhf_mod_package::{
    Candidate, Kind, Manifest, Resolved, Result, RuntimeConfig, Version, VersionReq,
};
use std::collections::BTreeSet;

/// Metadata for one Mod registered by an application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Builtin {
    id: &'static str,
    name: &'static str,
    dependencies: &'static [&'static str],
    default_enabled: bool,
}

pub const CONFIG: Builtin = Builtin {
    id: "mhf.config",
    name: "配置服务",
    dependencies: &[],
    default_enabled: false,
};
pub const DAT_REDIRECT: Builtin = Builtin {
    id: "mhf.dat-redirect",
    name: "DAT 文件重定向",
    dependencies: &[],
    default_enabled: false,
};
pub const BASE: Builtin = Builtin {
    id: "mhf.base",
    name: "基础支持",
    dependencies: &["mhf.config"],
    default_enabled: false,
};
pub const LOGIN: Builtin = Builtin {
    id: "mhf.login",
    name: "登录启动",
    dependencies: &["mhf.base", "mhf.config"],
    default_enabled: true,
};
pub const DEBUG: Builtin = Builtin {
    id: "mhf.debug",
    name: "调试启动",
    dependencies: &["mhf.base"],
    default_enabled: false,
};
pub const WORKBENCH: Builtin = Builtin {
    id: "mhf.workbench",
    name: "资源工作台",
    dependencies: &["mhf.base", "mhf.config"],
    default_enabled: false,
};

/// 根据调用应用的 Cargo features 选择条目。
/// cfg 在调用处展开，避免两个应用的依赖特性合并后相互污染各自公布的 Mod 清单。
#[macro_export]
macro_rules! builtin_catalog {
    () => {
        $crate::BuiltinCatalog::new(&[
            $crate::CONFIG,
            $crate::DAT_REDIRECT,
            #[cfg(feature = "base")]
            $crate::BASE,
            #[cfg(feature = "login")]
            $crate::LOGIN,
            #[cfg(feature = "debug")]
            $crate::DEBUG,
            #[cfg(feature = "workbench")]
            $crate::WORKBENCH,
        ])
    };
}

/// The application's registered entries, with no second set of feature flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinCatalog {
    entries: &'static [Builtin],
}

impl BuiltinCatalog {
    pub const fn new(entries: &'static [Builtin]) -> Self {
        Self { entries }
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>> {
        self.entries
            .iter()
            .map(|entry| {
                Candidate::builtin(Manifest {
                    schema: 1,
                    id: entry.id.into(),
                    name: entry.name.into(),
                    version: Version::new(1, 0, 0),
                    kind: Kind::Native,
                    entry: None,
                    dependencies: entry
                        .dependencies
                        .iter()
                        .map(|id| {
                            (
                                (*id).into(),
                                VersionReq::parse("^1.0").expect("fixed builtin range"),
                            )
                        })
                        .collect(),
                })
            })
            .collect()
    }

    /// Application-default roots; explicitly disabled Mods are excluded during resolution.
    pub fn defaults(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter(|entry| entry.default_enabled)
            .map(|entry| entry.id.into())
            .collect()
    }

    /// Application defaults only; a startup provider is selected by Mod configuration.
    pub fn resolve(&self, config: &RuntimeConfig, candidates: &[Candidate]) -> Result<Resolved> {
        mhf_mod_package::resolve(
            candidates,
            &config.selections(),
            &self.defaults(),
            &BTreeSet::new(),
        )
    }
}

#[cfg(test)]
mod tests;
