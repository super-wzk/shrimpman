//! Stage-specific resources consumed alongside ordinary FMOD/TXB packages.

mod area_camera;
pub mod hits;
mod keffect;
mod legacy_lighting;
mod legacy_render_tables;
mod lighting;
mod object_tables;
mod object_words;
mod objects;
mod placements;
pub mod render_tables;

pub use area_camera::{AreaCamera, AreaCameraCell, AreaCameraRegion};
pub use hits::{HitCell, HitRecord, Hits, HitsHeader};
pub use keffect::{KEffect, KEffectRecord};
pub use legacy_lighting::{
    LegacyLighting, LegacyLightingExtension, LegacyLightingRecord16, LegacyLightingRecord24,
    LegacyLightingTables,
};
pub use legacy_render_tables::LegacyRenderTables;
pub use lighting::{
    LightAnimation, LightAnimationChannel, LightCollision, Lighting, PostProcessing, Record,
    ToneMapping,
};
pub use object_tables::{ObjectTable, ObjectTables};
pub use object_words::ObjectWordTable;
pub use objects::{ObjectIndex, ObjectMember, ObjectPackage, ResolvedMember, ResourceReference};
pub use placements::{Placement, PlacementTable};
pub use render_tables::{MatchRecord, RenderTable, RenderTables};
