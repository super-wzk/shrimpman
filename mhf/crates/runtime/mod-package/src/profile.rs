use crate::{
    Candidate, Error, Kind, Manifest, Resolved, Result, RuntimeConfig, Source, Version, VersionReq,
};
use std::collections::BTreeSet;

/// Available built-in packages for a particular application build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinCatalog {
    pub login: bool,
    pub debug: bool,
}

impl BuiltinCatalog {
    pub fn candidates(&self) -> Result<Vec<Candidate>> {
        let mut descriptions = vec![
            ("mhf.config", "配置服务", vec![]),
            ("mhf.base", "基础支持", vec!["mhf.config"]),
        ];
        if self.login {
            descriptions.push(("mhf.login", "登录启动", vec!["mhf.config"]));
        }
        if self.debug {
            descriptions.push(("mhf.debug", "调试启动", vec!["mhf.base"]));
        }
        descriptions
            .into_iter()
            .map(|(id, name, dependencies)| {
                Candidate::builtin(Manifest {
                    schema: 1,
                    id: id.into(),
                    name: name.into(),
                    version: Version::new(1, 0, 0),
                    kind: Kind::Native,
                    entry: None,
                    dependencies: dependencies
                        .into_iter()
                        .map(|id| {
                            (
                                id.into(),
                                VersionReq::parse("^1.0").expect("fixed builtin range"),
                            )
                        })
                        .collect(),
                })
            })
            .collect()
    }

    /// Application defaults only; a startup provider is selected by Mod configuration.
    pub fn resolve(&self, config: &RuntimeConfig, candidates: &[Candidate]) -> Result<Resolved> {
        let mut defaults = BTreeSet::new();
        if self.login {
            defaults.insert("mhf.login".into());
        }
        let required = BTreeSet::from(["mhf.base".into()]);
        let resolved = crate::resolve(candidates, &config.selections(), &defaults, &required)?;
        let builtin = |id: &str| {
            resolved
                .mods
                .iter()
                .any(|candidate| candidate.manifest.id == id && candidate.source == Source::Builtin)
        };
        if builtin("mhf.debug") && !builtin("mhf.base") {
            return Err(Error::new(
                "当前内置调试界面使用 Base 的 egui 实现，需要内置 mhf.base",
            ));
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests;
