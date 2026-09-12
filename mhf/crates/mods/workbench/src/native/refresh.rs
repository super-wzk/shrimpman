//! Prepare edited resources before replacing the instances used by the renderer.

use super::*;
use crate::inspect::Document;
use std::path::Path;

pub(super) fn compatible_bindings(
    previous: &Arc<Vec<Option<usize>>>,
    count: usize,
) -> Arc<Vec<Option<usize>>> {
    if previous.len() == count && previous.iter().flatten().all(|&source| source < count) {
        return previous.clone();
    }
    Arc::new(
        (0..count)
            .map(|index| {
                previous
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|&source| source < count)
            })
            .collect(),
    )
}

pub(super) unsafe fn document(
    client: Client,
    runtime: &mut Runtime,
    path: &Path,
    document: Arc<Document>,
) -> Result<(), String> {
    runtime.fx.capture(&runtime.models);
    let fx = runtime.fx.refreshed(path, document.clone())?;
    let mut replacement = Runtime::default();
    let prepared = unsafe { prepare(client, runtime, path, document, &mut replacement) };
    if let Err(error) = prepared {
        unsafe {
            clear_models(client, &mut replacement)?;
            clear_skeletons(client, &mut replacement)?;
        }
        return Err(format!("预览保留编辑前的资源：{error}"));
    }

    // Construction and validation have completed with the original resource
    // buffers still alive. Native motions must go before their skeletons.
    for model in &mut runtime.models {
        if let Some(index) = replacement.models.iter().position(|new| new.id == model.id) {
            unsafe { model.release(client) }?;
            *model = replacement.models.remove(index);
        }
    }
    for skeleton in &mut runtime.skeletons {
        if let Some(index) = replacement
            .skeletons
            .iter()
            .position(|new| new.id == skeleton.id)
        {
            unsafe { skeleton.release(client) }?;
            *skeleton = replacement.skeletons.remove(index);
        }
    }
    for resource in &mut runtime.resources {
        if let Some(new) = replacement
            .resources
            .iter()
            .find(|new| new.id == resource.id)
        {
            *resource = new.clone();
        }
    }
    for motion in &mut runtime.motions {
        if let Some(new) = replacement.motions.iter().find(|new| new.id == motion.id) {
            *motion = new.clone();
        }
    }
    fx.detach_refreshed(&mut runtime.models, path);
    runtime.fx = fx;
    unsafe { reconcile_effects(client, runtime) }
}

unsafe fn prepare(
    client: Client,
    runtime: &Runtime,
    path: &Path,
    document: Arc<Document>,
    replacement: &mut Runtime,
) -> Result<(), String> {
    for resource in runtime
        .resources
        .iter()
        .filter(|entry| entry.source.belongs_to(path))
    {
        replacement.resources.push(LoadedResource {
            source: resource.source.remap(document.clone())?,
            ..resource.clone()
        });
    }
    for motion in runtime
        .motions
        .iter()
        .filter(|entry| entry.source.belongs_to(path))
    {
        let source = motion.source.remap(document.clone())?;
        let frames = animation::NativeMotion::duration(&source)?;
        replacement.motions.push(MotionResource {
            source,
            frames,
            frame: motion.frame.min(frames),
            ..motion.clone()
        });
    }
    let mut named_bundles = None;
    for model in runtime
        .models
        .iter()
        .filter(|entry| entry.definition.model.belongs_to(path))
    {
        let source = model.definition.model.remap(document.clone())?;
        let named =
            named_bundles.get_or_insert_with(|| AssetBundle::find_with_nodes(document.clone()).0);
        let definition = AssetBundle::from_source(source, named);
        let bundle = definition.loaded_from(&replacement.resources);
        let (asset, target) = unsafe { model.prepare_resources(client, &bundle) }?;
        replacement.models.push(Model {
            id: model.id,
            identity: definition.model.equipment_model(),
            definition,
            bundle,
            asset: Some(asset),
            visible: model.visible,
            error: None,
            motions: Vec::new(),
            frame: model.frame,
            bones: Arc::default(),
            effects: Effects {
                bindings: model.effects.bindings.clone(),
                target,
                snapshot: Arc::default(),
            },
        });
    }
    for skeleton in runtime
        .skeletons
        .iter()
        .filter(|entry| entry.source.belongs_to(path))
    {
        replacement.skeletons.push(SkeletonResource {
            id: skeleton.id,
            source: skeleton.source.remap(document.clone())?,
            native: None,
            motions: Vec::new(),
            bones: Arc::default(),
            bindings: skeleton.bindings.clone(),
            error: None,
        });
    }
    unsafe { reconcile_skeletons(client, replacement) }?;
    if let Some(error) = replacement
        .skeletons
        .iter()
        .find_map(|skeleton| skeleton.error.as_deref())
    {
        return Err(error.to_owned());
    }
    unsafe { reconcile_motions(client, replacement) }?;
    for motion in &replacement.motions {
        let bound = |runtime: &Runtime| {
            runtime
                .models
                .iter()
                .flat_map(|model| &model.motions)
                .chain(
                    runtime
                        .skeletons
                        .iter()
                        .flat_map(|skeleton| &skeleton.motions),
                )
                .any(|bound| bound.resource == motion.id)
        };
        if bound(runtime) && !bound(replacement) {
            return Err(format!(
                "编辑后的动画无法绑定原骨架：{}",
                motion.source.short_name()
            ));
        }
    }
    Ok(())
}
