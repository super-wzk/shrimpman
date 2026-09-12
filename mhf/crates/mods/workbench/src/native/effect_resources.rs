//! Loaded effect sources outlive model selection and native model instances.

use super::{Client, Model};
use crate::preview::{
    LoadedEffect, ResourceRef,
    effects::{Binding, BindingSnapshot, Effects, Target},
};
use mhf_resource::{dat, effect::ModelEffectBinding};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Mode {
    Automatic,
    Manual(Option<u64>),
}

struct Resource {
    binding: Binding,
    metadata: Option<ModelEffectBinding>,
    enabled: bool,
    mode: Mode,
    model: Option<u64>,
    clock: f32,
    restart: bool,
    message: Arc<str>,
}

impl Resource {
    fn select_model(
        &self,
        ready: &[u64],
        matches: impl Fn(&ModelEffectBinding, u64) -> bool,
    ) -> (Option<u64>, String) {
        if !self.enabled {
            return (None, "已禁用".into());
        }
        match self.mode {
            Mode::Manual(None) => (None, "未绑定模型".into()),
            Mode::Manual(Some(id)) => {
                if ready.contains(&id) {
                    (Some(id), "手动绑定".into())
                } else {
                    (None, "所选模型尚未可用".into())
                }
            }
            Mode::Automatic => {
                let Some(metadata) = &self.metadata else {
                    return (None, "此资源没有模型 ID，请手动选择模型".into());
                };
                let mut targets = ready.iter().copied().filter(|&id| matches(metadata, id));
                match (targets.next(), targets.next()) {
                    (Some(id), None) => (Some(id), "自动绑定".into()),
                    (None, _) => (
                        None,
                        format!("等待模型 ID {} 对应的模型", metadata.model_id),
                    ),
                    _ => (
                        None,
                        format!("模型 ID {} 对应多个模型，请手动选择", metadata.model_id),
                    ),
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Registry {
    entries: Vec<Resource>,
}

impl Registry {
    pub fn load(&mut self, source: ResourceRef) -> Result<u64, String> {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.binding.source.same_source(&source))
        {
            entry.enabled = true;
            entry.restart = true;
            return Ok(entry.binding.id);
        }
        let binding = Binding::read(source)?;
        // Only DAT 165 carries a model ID. A definition selected from beneath a
        // binding is still an independent definition, without inherited IDs.
        let metadata = if matches!(
            binding.source.kind(),
            crate::inspect::Kind::DatRecord(index) if index == dat::DATA_TABLES.len() + 2
        ) {
            Some(
                ModelEffectBinding::parse(binding.source.bytes()?)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        let id = binding.id;
        self.entries.push(Resource {
            binding,
            metadata,
            enabled: true,
            mode: Mode::Automatic,
            model: None,
            clock: 0.0,
            restart: true,
            message: "未绑定模型".into(),
        });
        Ok(id)
    }

    pub fn set_target(&mut self, binding: u64, model: Option<u64>) -> Result<(), String> {
        self.entry_mut(binding)?.mode = Mode::Manual(model);
        Ok(())
    }

    pub fn set_automatic(&mut self, binding: u64) -> Result<(), String> {
        self.entry_mut(binding)?.mode = Mode::Automatic;
        Ok(())
    }

    pub fn set_enabled(&mut self, binding: u64, enabled: bool) -> Result<(), String> {
        self.entry_mut(binding)?.enabled = enabled;
        Ok(())
    }

    fn entry_mut(&mut self, binding: u64) -> Result<&mut Resource, String> {
        self.entries
            .iter_mut()
            .find(|entry| entry.binding.id == binding)
            .ok_or_else(|| "特效资源已卸载".into())
    }

    /// Preserve playback before removing/replacing native model instances.
    pub fn capture(&mut self, models: &[Model]) {
        for entry in &mut self.entries {
            let Some(id) = entry.model else {
                continue;
            };
            let current = models
                .iter()
                .find(|model| model.id == id)
                .and_then(|model| {
                    model
                        .effects
                        .bindings
                        .iter()
                        .find(|binding| binding.id == entry.binding.id)
                        .map(|binding| (model.frame, binding))
                });
            match current {
                Some((clock, binding)) => {
                    entry.binding = binding.clone();
                    entry.clock = clock;
                }
                None => {
                    entry.model = None;
                    entry.message = "未绑定模型".into();
                }
            }
        }
    }

    /// Automatic matching is supplied by the native model identity resolver;
    /// the registry never guesses identities from names or active selection.
    pub unsafe fn reconcile(
        &mut self,
        client: Client,
        models: &mut [Model],
        matches: impl Fn(&ModelEffectBinding, &Model) -> bool,
    ) -> Result<(), String> {
        self.capture(models);
        let ready = models
            .iter()
            .filter(|model| model.asset.is_some() && model.error.is_none())
            .map(|model| model.id)
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for entry in &mut self.entries {
            let (selected, message) = entry.select_model(&ready, |metadata, id| {
                models
                    .iter()
                    .find(|model| model.id == id)
                    .is_some_and(|model| matches(metadata, model))
            });
            entry.message = message.into();
            let attached = selected == entry.model
                && selected.is_some_and(|id| {
                    models.iter().any(|model| {
                        model.id == id
                            && model
                                .effects
                                .bindings
                                .iter()
                                .any(|binding| binding.id == entry.binding.id)
                    })
                });
            if attached {
                if entry.restart {
                    let model = models
                        .iter_mut()
                        .find(|model| Some(model.id) == selected)
                        .unwrap();
                    if let Err(error) = restart(entry, model) {
                        entry.message = error.clone().into();
                        errors.push(error);
                    }
                }
                continue;
            }
            detach(models, entry.binding.id);
            entry.model = None;
            let Some(model) = models.iter_mut().find(|model| Some(model.id) == selected) else {
                continue;
            };
            let target = unsafe {
                model
                    .asset
                    .as_ref()
                    .expect("selected model is ready")
                    .effect_target(client)
            };
            if let Err(error) = target.and_then(|target| attach(entry, model, target)) {
                detach(std::slice::from_mut(model), entry.binding.id);
                entry.model = None;
                entry.message = error.clone().into();
                errors.push(error);
            }
        }
        self.capture(models);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    fn owner_mut<'a>(
        &self,
        models: &'a mut [Model],
        binding: u64,
    ) -> Result<&'a mut Model, String> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.binding.id == binding)
            .ok_or("特效资源已卸载")?;
        if !entry.enabled {
            return Err("请先启用此特效资源".into());
        }
        let id = entry.model.ok_or("请先为特效选择模型")?;
        models
            .iter_mut()
            .find(|model| model.id == id)
            .ok_or_else(|| "特效关联模型已卸载".into())
    }

    pub fn trigger(
        &mut self,
        models: &mut [Model],
        binding: u64,
        slot: usize,
    ) -> Result<(), String> {
        let model = self.owner_mut(models, binding)?;
        model.effects.trigger(binding, slot, model.frame)?;
        refresh(model);
        self.capture(models);
        Ok(())
    }

    pub fn stop(&mut self, models: &mut [Model], binding: u64, slot: usize) -> Result<(), String> {
        let model = self.owner_mut(models, binding)?;
        model.effects.stop(binding, slot)?;
        refresh(model);
        self.capture(models);
        Ok(())
    }

    pub fn seek(
        &mut self,
        models: &mut [Model],
        binding: u64,
        slot: usize,
        frame: f32,
    ) -> Result<(), String> {
        let model = self.owner_mut(models, binding)?;
        model.effects.seek(binding, slot, model.frame, frame)?;
        refresh(model);
        self.capture(models);
        Ok(())
    }

    pub fn step(
        &mut self,
        models: &mut [Model],
        binding: u64,
        slot: usize,
        delta: i8,
    ) -> Result<(), String> {
        let model = self.owner_mut(models, binding)?;
        model.effects.step(binding, slot, model.frame, delta)?;
        refresh(model);
        self.capture(models);
        Ok(())
    }

    pub fn remove_definition(
        &mut self,
        models: &mut [Model],
        binding: u64,
        slot: usize,
    ) -> Result<(), String> {
        self.capture(models);
        let entry = self.entry_mut(binding)?;
        let index = entry
            .binding
            .entries
            .iter()
            .position(|entry| entry.slot == slot)
            .ok_or("特效定义已移除")?;
        entry.binding.entries.remove(index);
        if entry.binding.entries.is_empty() {
            return self.remove(models, binding);
        }
        for model in models {
            if let Some(source) = model
                .effects
                .bindings
                .iter_mut()
                .find(|source| source.id == binding)
            {
                source.entries.retain(|entry| entry.slot != slot);
                refresh(model);
            }
        }
        Ok(())
    }

    pub fn remove(&mut self, models: &mut [Model], binding: u64) -> Result<(), String> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.binding.id == binding)
            .ok_or("特效资源已卸载")?;
        detach(models, binding);
        self.entries.remove(index);
        Ok(())
    }

    pub fn clear(&mut self, models: &mut [Model]) {
        for model in models {
            model.effects.bindings.clear();
            refresh(model);
        }
        self.entries.clear();
    }

    pub fn snapshot(&mut self, models: &[Model]) -> Vec<LoadedEffect> {
        self.capture(models);
        self.entries
            .iter()
            .map(|entry| {
                let model = entry
                    .model
                    .and_then(|id| models.iter().find(|model| model.id == id));
                let snapshot = model.and_then(|model| {
                    model
                        .effects
                        .snapshot
                        .iter()
                        .find(|binding| binding.id == entry.binding.id)
                        .cloned()
                });
                LoadedEffect {
                    id: entry.binding.id,
                    source: entry.binding.source.clone(),
                    enabled: entry.enabled,
                    model: model.map(|model| model.id),
                    manual_target: match entry.mode {
                        Mode::Automatic => None,
                        Mode::Manual(model) => model,
                    },
                    automatic: matches!(entry.mode, Mode::Automatic),
                    model_id: entry.metadata.as_ref().map(|metadata| metadata.model_id),
                    message: entry.message.clone(),
                    binding: snapshot.unwrap_or_else(|| unbound_snapshot(entry)),
                }
            })
            .collect()
    }
}

fn refresh(model: &mut Model) {
    model.effects.snapshot = Arc::new(model.effects.sample(model.frame).bindings);
}

fn detach(models: &mut [Model], binding: u64) {
    for model in models {
        let before = model.effects.bindings.len();
        model.effects.bindings.retain(|source| source.id != binding);
        if before != model.effects.bindings.len() {
            refresh(model);
        }
    }
}

fn restart(entry: &mut Resource, model: &mut Model) -> Result<(), String> {
    for slot in entry.binding.entries.iter().map(|entry| entry.slot) {
        model.effects.trigger(entry.binding.id, slot, model.frame)?;
    }
    entry.restart = false;
    refresh(model);
    Ok(())
}

fn attach(entry: &mut Resource, model: &mut Model, target: Target) -> Result<(), String> {
    let mut binding = entry.binding.clone();
    let delta = model.frame - entry.clock;
    for definition in &mut binding.entries {
        if let Some(start) = &mut definition.started_at {
            *start += delta;
        }
    }
    model.effects.target = target;
    model.effects.bindings.push(binding);
    if entry.restart {
        restart(entry, model)?;
    } else {
        // Resuming a running definition applies the same conflict rules as a
        // trigger, then restores its local playback position in the new clock.
        for definition in &entry.binding.entries {
            let Some(start) = definition.started_at else {
                continue;
            };
            let slot = definition.slot;
            model.effects.trigger(entry.binding.id, slot, model.frame)?;
            model
                .effects
                .bindings
                .iter_mut()
                .find(|binding| binding.id == entry.binding.id)
                .unwrap()
                .entries
                .iter_mut()
                .find(|entry| entry.slot == slot)
                .unwrap()
                .started_at = Some(start + delta);
        }
        refresh(model);
    }
    entry.model = Some(model.id);
    entry.clock = model.frame;
    Ok(())
}

fn unbound_snapshot(entry: &Resource) -> BindingSnapshot {
    let effects = Effects {
        bindings: vec![entry.binding.clone()],
        ..Effects::default()
    };
    let mut snapshot = effects.sample(entry.clock).bindings.remove(0);
    for definition in &mut snapshot.definitions {
        definition.active = false;
        definition.frame = None;
        definition.position = None;
        definition.message = entry.message.to_string();
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{AssetBundle, effects::tests::fixture};

    fn model(id: u64, frame: f32, source: ResourceRef) -> Model {
        let definition = AssetBundle {
            model: source,
            skeleton: None,
            textures: Vec::new(),
            name: format!("model {id}"),
        };
        Model {
            id,
            identity: None,
            bundle: definition.clone(),
            definition,
            asset: None,
            visible: true,
            error: None,
            motions: Vec::new(),
            frame,
            bones: Arc::default(),
            effects: Effects::default(),
        }
    }

    fn target() -> Target {
        Target {
            nodes: 4,
            material_counts: vec![1, 3],
        }
    }

    #[test]
    fn only_a_binding_with_its_own_model_id_can_select_an_automatic_target() {
        let (attachment, source) = fixture();
        let expanded = Arc::new(crate::inspect::expand(&source.document, source.node).unwrap());
        let definition = ResourceRef {
            node: expanded.nodes[source.node].children[0],
            document: expanded,
        };
        let mut registry = Registry::default();
        let attachment_id = registry.load(attachment).unwrap();
        let binding_id = registry.load(source.clone()).unwrap();
        let definition_id = registry.load(definition).unwrap();
        assert_eq!(registry.load(source).unwrap(), binding_id);
        assert_eq!(registry.entries.len(), 3);
        let binding = registry.entry_mut(binding_id).unwrap();
        let metadata = binding.metadata.as_ref().unwrap();
        assert_eq!(
            (
                metadata.part_code,
                metadata.weapon_class,
                metadata.variant,
                metadata.model_id
            ),
            (3, 7, 2, 44)
        );
        assert_eq!(binding.select_model(&[41], |_, _| true).0, Some(41));
        assert_eq!(binding.select_model(&[41, 42], |_, _| true).0, None);
        assert_eq!(
            binding.select_model(&[41, 42], |_, id| id == 42).0,
            Some(42)
        );
        for id in [attachment_id, definition_id] {
            let entry = registry.entry_mut(id).unwrap();
            assert!(entry.metadata.is_none());
            assert_eq!(entry.select_model(&[41], |_, _| true).0, None);
        }
        assert!(registry.snapshot(&[]).iter().all(|entry| {
            entry.model.is_none()
                && entry.binding.definitions.iter().all(|definition| {
                    definition.frame.is_none()
                        && !definition.active
                        && definition.position.is_none()
                })
        }));
    }

    #[test]
    fn manual_target_and_unbind_override_automatic_matching_until_explicitly_reset() {
        let (_, source) = fixture();
        let mut registry = Registry::default();
        let id = registry.load(source).unwrap();
        registry.set_target(id, Some(99)).unwrap();
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[41, 99], |_, id| id == 41)
                .0,
            Some(99)
        );
        // Removing the requested model must not redirect to an automatic match.
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[41], |_, _| true)
                .0,
            None
        );
        registry.set_enabled(id, false).unwrap();
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[99], |_, _| true)
                .0,
            None
        );
        registry.set_enabled(id, true).unwrap();
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[99], |_, _| false)
                .0,
            Some(99)
        );
        registry.set_target(id, None).unwrap();
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[41], |_, _| true)
                .0,
            None
        );
        assert!(!registry.snapshot(&[])[0].automatic);
        registry.set_automatic(id).unwrap();
        assert_eq!(
            registry
                .entry_mut(id)
                .unwrap()
                .select_model(&[41], |_, _| true)
                .0,
            Some(41)
        );
        assert!(registry.snapshot(&[])[0].automatic);
    }

    #[test]
    fn released_models_clear_only_the_resolved_owner_and_recreation_keeps_the_effect_clock() {
        let (_, source) = fixture();
        let mut registry = Registry::default();
        let id = registry.load(source.clone()).unwrap();
        registry.set_target(id, Some(41)).unwrap();
        let mut models = vec![model(41, 100.0, source.clone())];
        attach(registry.entry_mut(id).unwrap(), &mut models[0], target()).unwrap();
        registry.seek(&mut models, id, 0, 15.0).unwrap();

        // Native release can complete before replacement succeeds or before
        // reconciliation runs. The loaded model ID alone is not a binding.
        registry.capture(&models);
        models[0].effects = Effects::default();
        let detached = registry.snapshot(&models).remove(0);
        assert_eq!(detached.model, None);
        assert_eq!(detached.manual_target, Some(41));
        assert!(detached.enabled);
        assert!(!detached.automatic);
        assert_eq!(detached.binding.definitions[0].frame, None);
        assert!(registry.trigger(&mut models, id, 0).is_err());

        models[0].frame = 0.0;
        attach(registry.entry_mut(id).unwrap(), &mut models[0], target()).unwrap();
        let rebound = registry.snapshot(&models).remove(0);
        assert_eq!(rebound.model, Some(41));
        assert_eq!(rebound.binding.definitions[0].frame, Some(15.0));

        registry.capture(&models);
        models.clear();
        let unloaded = registry.snapshot(&models).remove(0);
        assert_eq!(unloaded.model, None);
        assert_eq!(unloaded.manual_target, Some(41));
        assert!(unloaded.source.same_source(&source));
        registry.set_target(id, Some(99)).unwrap();
        models.push(model(99, 8.0, source));
        attach(registry.entry_mut(id).unwrap(), &mut models[0], target()).unwrap();
        let redirected = registry.snapshot(&models).remove(0);
        assert_eq!(redirected.model, Some(99));
        assert_eq!(redirected.manual_target, Some(99));
        assert_eq!(redirected.binding.definitions[0].frame, Some(15.0));
    }

    #[test]
    fn commands_rebinding_and_disabling_preserve_the_binding_clock_and_conflict_state() {
        let (_, first) = fixture();
        let (_, second) = fixture();
        let mut registry = Registry::default();
        let a = registry.load(first.clone()).unwrap();
        let b = registry.load(second.clone()).unwrap();
        let mut models = vec![model(41, 100.0, first), model(99, 400.0, second)];
        attach(registry.entry_mut(a).unwrap(), &mut models[0], target()).unwrap();
        attach(registry.entry_mut(b).unwrap(), &mut models[1], target()).unwrap();
        registry.seek(&mut models, a, 0, 5.0).unwrap();
        registry.seek(&mut models, b, 0, 17.0).unwrap();
        registry.step(&mut models, a, 0, 1).unwrap();
        let frame = |registry: &mut Registry, models: &[Model], id| {
            registry
                .snapshot(models)
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap()
                .binding
                .definitions[0]
                .frame
        };
        assert_eq!(frame(&mut registry, &models, a), Some(6.0));
        assert_eq!(frame(&mut registry, &models, b), Some(17.0));
        registry.stop(&mut models, b, 0).unwrap();
        assert_eq!(frame(&mut registry, &models, a), Some(6.0));
        assert_eq!(frame(&mut registry, &models, b), None);
        registry.trigger(&mut models, b, 0).unwrap();
        assert_eq!(frame(&mut registry, &models, b), Some(0.0));

        registry.set_target(a, Some(99)).unwrap();
        registry.capture(&models);
        detach(&mut models, a);
        attach(registry.entry_mut(a).unwrap(), &mut models[1], target()).unwrap();
        assert!(models[0].effects.bindings.is_empty());
        assert_eq!(frame(&mut registry, &models, a), Some(6.0));
        assert_eq!(
            frame(&mut registry, &models, b),
            None,
            "resuming applies target conflicts"
        );

        registry.set_enabled(a, false).unwrap();
        registry.capture(&models);
        detach(&mut models, a);
        registry.entry_mut(a).unwrap().model = None;
        models[1].frame = 450.0;
        assert_eq!(frame(&mut registry, &models, a), None);
        registry.set_enabled(a, true).unwrap();
        attach(registry.entry_mut(a).unwrap(), &mut models[1], target()).unwrap();
        assert_eq!(frame(&mut registry, &models, a), Some(6.0));
        assert_eq!(frame(&mut registry, &models, b), None);
        registry.remove_definition(&mut models, a, 0).unwrap();
        assert_eq!(registry.snapshot(&models).len(), 1);
        assert_eq!(models[1].effects.bindings.len(), 1);
        assert_eq!(models[1].effects.bindings[0].id, b);
        assert!(registry.seek(&mut models, a, 0, 1.0).is_err());
        registry.clear(&mut models);
        assert!(registry.snapshot(&models).is_empty());
        assert!(models.iter().all(|model| model.effects.bindings.is_empty()));
    }
}
