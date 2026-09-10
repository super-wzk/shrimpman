use crate::{Candidate, Kind, Manifest, Resolved, Result, RuntimeConfig, Version, VersionReq};
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
            descriptions.push(("mhf.login", "登录启动", vec!["mhf.base", "mhf.config"]));
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

    /// Application-default roots; explicitly disabled Mods are excluded during resolution.
    pub fn defaults(&self) -> BTreeSet<String> {
        let mut defaults = BTreeSet::new();
        if self.login {
            defaults.insert("mhf.login".into());
        }
        defaults
    }

    /// Application defaults only; a startup provider is selected by Mod configuration.
    pub fn resolve(&self, config: &RuntimeConfig, candidates: &[Candidate]) -> Result<Resolved> {
        crate::resolve(
            candidates,
            &config.selections(),
            &self.defaults(),
            &BTreeSet::new(),
        )
    }
}

#[cfg(test)]
mod tests;
