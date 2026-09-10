//! FMOD geometry with 32-bit vertex indices and draw-batch lengths.
//! The native adapter targets the verified ZZ HD client; resource files retain
//! their existing format, vertex conversion, materials and animation data.

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF geometry adapter requires i686 Windows");

#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
mod equipment_cache;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
mod fmod;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
mod mesh;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod native;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
mod patches;

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod module;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use module::GeometryMod;

#[cfg(test)]
mod tests;
