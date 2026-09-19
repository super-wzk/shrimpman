//! Monster domain for the verified ZZ HD client.
//!
//! The crate owns two independent features and one place that aggregates them
//! into a single `MonsterMod` for the host:
//!
//! * [`ai`] — the monster AI feature: the pointer-free graph, the author
//!   language and the binding that installs a compiled graph onto the live
//!   actor, plus the `dat/monster-ai` overlay. The graph and the compiler are
//!   portable; only the binding needs the provider targets.
//! * `species` — the species-limit patches: eight verified upper-bound checks
//!   raised from 177 to 255.
//!
//! `native` holds what both halves share: typed access to the game image and
//! the fingerprint of the supported build. Neither feature knows about the
//! other; only `MonsterMod` decides the order they attach, roll back and detach
//! in — species first, so a rejected overlay leaves no half-attached adapter
//! behind.
//!
//! The author-facing text form is [`ai::dsl`]: `parse` reads a document,
//! `Document::compile` builds the graph, and the binding layers a
//! `base native;` declaration onto the live block it is about to replace.
//! `docs/dsl-spec.md` owns the language. These patches alone do not register
//! additional species, and an overlay file only replaces the selection block of
//! a species the client already has.

/// The monster AI feature: the graph, the compiler and the binding.
///
/// The DLL has exactly one way in — `MonsterMod` — so in the game build this
/// module is private and the compiler reports whatever the game never reaches.
/// On every other target `MonsterMod` does not exist, so the module is public
/// and the tests are what keep the tree alive.
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod ai;
#[cfg(not(all(feature = "provider", windows, target_arch = "x86")))]
pub mod ai;

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF monster adapter requires i686 Windows");

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod native;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod species;

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
use self::{ai::overlay::Hook, species::Patches};
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
use mhf_mod_host::{Context, Module, Result as HostResult};
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
use windows::Win32::Foundation::HMODULE;

/// Both monster features under one lifecycle.
///
/// Attach writes the species patches before installing the AI hook, so a hook
/// that refuses the image rolls the patches back instead of leaving half an
/// adapter behind. Detach runs in the opposite order, and every step keeps its
/// state for a retry until `prepare_release` hands the module reference back.
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
#[derive(Default)]
pub struct MonsterMod {
    patches: Option<Patches>,
    overlay: Option<Hook>,
}

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
impl Module for MonsterMod {
    fn attach(&mut self, context: &Context) -> HostResult<()> {
        if self.patches.is_some() || self.overlay.is_some() {
            return Err("monster patches are already attached".into());
        }
        let module = HMODULE(context.game().module_base);
        let patches = unsafe { Patches::prepare(module)? };
        // Base must own the state before a partial write can fail.
        self.patches.insert(patches).apply()?;
        match unsafe { Hook::install(module) } {
            Ok(overlay) => {
                self.overlay = Some(overlay);
                Ok(())
            }
            // A half-attached adapter is worse than a rejected one: the species
            // patches go back before the error reaches the host.
            Err(error) => {
                let rollback = self.patches.as_mut().map(Patches::restore);
                Err(match rollback {
                    Some(Err(cleanup)) => format!("{error}; rollback also failed: {cleanup}"),
                    _ => error,
                })
            }
        }
    }

    fn detach(&mut self, _context: &Context) -> HostResult<()> {
        if let Some(overlay) = &mut self.overlay {
            overlay.uninstall()?;
        }
        if let Some(patches) = &mut self.patches {
            patches.restore()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> HostResult<()> {
        if let Some(overlay) = &mut self.overlay {
            unsafe { overlay.prepare_release() }?;
        }
        if let Some(patches) = &self.patches {
            unsafe { patches.prepare_release() }?;
        }
        Ok(())
    }
}
