//! Keep a weapon's texture identities independent of mutable native bank slots.

use std::collections::HashMap;

pub(crate) const HANDLE_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Texture {
    pub handle: u32,
    pub generation: u64,
    // Zero while GPU creation is pending; an actual COM identity afterwards.
    pub pointer: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnerInfo {
    pub resource: usize,
    pub base: u32,
    pub count: u32,
    pub player: u16,
    pub weapon: u16,
    pub model: u16,
}

struct Owner {
    info: OwnerInfo,
    constructing: bool,
    retired: bool,
    textures: Vec<Texture>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Token(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CreationTicket {
    owner: Token,
    handle: u32,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CreationResult {
    Published,
    Retired(Texture),
    Discarded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReleaseClaim {
    handle: u32,
    generation: u64,
    identity: u64,
}

pub(crate) struct Ownership {
    generations: Vec<u64>,
    next_generation: u64,
    next_owner: u64,
    next_release: u64,
    owners: HashMap<Token, Owner>,
    resources: HashMap<usize, Token>,
    releasing: Vec<Option<ReleaseClaim>>,
}

impl Default for Ownership {
    fn default() -> Self {
        Self {
            generations: vec![0; HANDLE_LIMIT],
            next_generation: 0,
            next_owner: 0,
            next_release: 0,
            owners: HashMap::new(),
            resources: HashMap::new(),
            releasing: vec![None; HANDLE_LIMIT],
        }
    }
}

impl Ownership {
    pub(crate) fn generation(&self, handle: u32) -> Option<u64> {
        self.generations.get(handle as usize).copied()
    }

    /// Every native reservation starts a new identity, even if COM reuses an address.
    pub(crate) fn reserve(&mut self, handle: u32) {
        if handle == 0 || handle as usize >= HANDLE_LIMIT {
            return;
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        self.generations[handle as usize] = self.next_generation;
        // A native reservation can only reuse a slot after its previous Release
        // has cleared it. The previous callback may still be returning to Rust.
        self.releasing[handle as usize] = None;
    }

    pub(crate) fn begin(&mut self, info: OwnerInfo) -> (Token, Vec<Texture>) {
        let retired = self.take(info.resource);
        self.next_owner = self.next_owner.wrapping_add(1);
        let token = Token(self.next_owner);
        self.owners.insert(
            token,
            Owner {
                info,
                constructing: true,
                retired: false,
                textures: Vec::new(),
            },
        );
        self.resources.insert(info.resource, token);
        (token, retired)
    }

    pub(crate) fn finish(&mut self, token: Token) {
        if let Some(owner) = self.owners.get_mut(&token) {
            owner.constructing = false;
        }
        self.remove_finished(token);
    }

    /// Register before the native creation callback can yield to another thread.
    pub(crate) fn pending(&mut self, token: Token, handle: u32) -> Option<CreationTicket> {
        if handle == 0 || handle as usize >= HANDLE_LIMIT {
            return None;
        }
        let owner = self.owners.get_mut(&token)?;
        let generation = self.generations[handle as usize];
        if !owner
            .textures
            .iter()
            .any(|texture| texture.handle == handle && texture.generation == generation)
        {
            owner.textures.push(Texture {
                handle,
                generation,
                pointer: 0,
            });
        }
        Some(CreationTicket {
            owner: token,
            handle,
            generation,
        })
    }

    /// Called after the complete outer native creator, including GetDesc/LockRect.
    /// A retired creation remains protected until this point, then its completed
    /// identity is returned for release through the native synchronous bridge.
    /// Obsolete tickets must not publish a handle that now names another texture.
    pub(crate) fn created(&mut self, ticket: CreationTicket, pointer: usize) -> CreationResult {
        let current = self.generation(ticket.handle) == Some(ticket.generation);
        let Some(owner) = self.owners.get_mut(&ticket.owner) else {
            return CreationResult::Discarded;
        };
        let Some(index) = owner.textures.iter().position(|texture| {
            texture.handle == ticket.handle && texture.generation == ticket.generation
        }) else {
            return CreationResult::Discarded;
        };
        let result = if !current || pointer == 0 || pointer == u32::MAX as usize {
            owner.textures.remove(index);
            CreationResult::Discarded
        } else if owner.retired {
            let mut texture = owner.textures.remove(index);
            texture.pointer = pointer;
            CreationResult::Retired(texture)
        } else {
            owner.textures[index].pointer = pointer;
            CreationResult::Published
        };
        self.remove_finished(ticket.owner);
        result
    }

    pub(crate) fn protects(&self, handle: u32, pointer: usize) -> Option<OwnerInfo> {
        let &generation = self.generations.get(handle as usize)?;
        self.owners.values().find_map(|owner| {
            owner
                .textures
                .iter()
                .any(|texture| {
                    texture.handle == handle
                        && texture.generation == generation
                        && (texture.pointer == 0 || texture.pointer == pointer)
                })
                .then_some(owner.info)
        })
    }

    pub(crate) fn take(&mut self, resource: usize) -> Vec<Texture> {
        self.resources
            .remove(&resource)
            .map_or_else(Vec::new, |token| self.retire(token))
    }

    pub(crate) fn take_all(&mut self) -> Vec<Texture> {
        let mut textures = Vec::new();
        self.resources.clear();
        let tokens = self.owners.keys().copied().collect::<Vec<_>>();
        for token in tokens {
            for texture in self.retire(token) {
                if !textures.contains(&texture) {
                    textures.push(texture);
                }
            }
        }
        textures
    }

    fn retire(&mut self, token: Token) -> Vec<Texture> {
        let mut textures = Vec::new();
        if let Some(owner) = self.owners.get_mut(&token) {
            owner.retired = true;
            owner.textures.retain(|texture| {
                if texture.pointer == 0 {
                    // Keep protecting the native creator while it is waiting on
                    // its callback or reading the COM object after the callback.
                    true
                } else {
                    textures.push(*texture);
                    false
                }
            });
        }
        self.remove_finished(token);
        textures
    }

    fn remove_finished(&mut self, token: Token) {
        if self
            .owners
            .get(&token)
            .is_some_and(|owner| owner.retired && !owner.constructing && owner.textures.is_empty())
        {
            self.owners.remove(&token);
        }
    }

    /// Serialize releases for one handle without holding this mutex while native
    /// code runs. Both ordinary native cleanup and retired snapshots must claim.
    pub(crate) fn claim_release(
        &mut self,
        handle: u32,
        pointer: usize,
        expected: Option<Texture>,
    ) -> Option<ReleaseClaim> {
        if handle == 0
            || handle as usize >= HANDLE_LIMIT
            || pointer == 0
            || pointer == u32::MAX as usize
            || self.releasing[handle as usize].is_some()
            || self.protects(handle, pointer).is_some()
            || expected.is_some_and(|texture| {
                texture.handle != handle
                    || texture.pointer != pointer
                    || texture.generation != self.generations[handle as usize]
            })
        {
            return None;
        }
        self.next_release = self.next_release.wrapping_add(1);
        let claim = ReleaseClaim {
            handle,
            generation: self.generations[handle as usize],
            identity: self.next_release,
        };
        self.releasing[handle as usize] = Some(claim);
        Some(claim)
    }

    pub(crate) fn finish_release(&mut self, claim: ReleaseClaim) {
        if self.releasing[claim.handle as usize] == Some(claim) {
            self.releasing[claim.handle as usize] = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(resource: usize, base: u32, count: u32) -> OwnerInfo {
        OwnerInfo {
            resource,
            base,
            count,
            player: 0,
            weapon: 0,
            model: 0,
        }
    }

    fn create(ownership: &mut Ownership, token: Token, handle: u32, pointer: usize) {
        ownership.reserve(handle);
        let ticket = ownership.pending(token, handle).unwrap();
        assert_eq!(
            ownership.created(ticket, pointer),
            CreationResult::Published
        );
    }

    #[test]
    fn adjacent_bank_cleanup_does_not_destroy_the_other_weapon() {
        let mut ownership = Ownership::default();
        let first = ownership.begin(owner(10, 220, 2)).0;
        create(&mut ownership, first, 1, 101);
        create(&mut ownership, first, 2, 102);
        ownership.finish(first);
        let second = ownership.begin(owner(20, 221, 1)).0;
        create(&mut ownership, second, 3, 103);
        ownership.finish(second);
        let retired = ownership.take(10);
        assert!(ownership.protects(1, 101).is_none());
        let protected = ownership.protects(3, 103).unwrap();
        assert_eq!(
            (protected.resource, protected.base, protected.count),
            (20, 221, 1)
        );
        assert_eq!(
            (protected.player, protected.weapon, protected.model),
            (0, 0, 0)
        );
        // Native cleanup finds 1 and 3 in the overwritten bank. Snapshot cleanup
        // instead releases 1 and the orphaned 2, keeping the live weapon's 3.
        assert_eq!(
            retired
                .iter()
                .map(|texture| texture.handle)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        for texture in retired {
            assert!(
                ownership
                    .claim_release(texture.handle, 100 + texture.handle as usize, Some(texture))
                    .is_some()
            );
        }
    }

    #[test]
    fn pending_creation_is_protected_before_the_com_pointer_is_published() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(token, 1).unwrap();
        assert_eq!(ownership.protects(1, 101).unwrap().resource, 10);
        assert_eq!(ownership.created(ticket, 101), CreationResult::Published);
        ownership.finish(token);
        assert!(ownership.protects(1, 999).is_none());
        assert!(ownership.protects(1, 101).is_some());
    }

    #[test]
    fn recycled_handle_and_com_address_do_not_match_a_retired_generation() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        create(&mut ownership, token, 1, 101);
        ownership.finish(token);
        let old = ownership.take(10)[0];
        ownership.reserve(1);
        assert_ne!(ownership.generation(1), Some(old.generation));
        assert!(ownership.claim_release(1, 101, Some(old)).is_none());
    }

    #[test]
    fn failed_creation_does_not_leave_a_live_owner_reference() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(token, 1).unwrap();
        assert_eq!(ownership.created(ticket, 0), CreationResult::Discarded);
        ownership.finish(token);
        assert!(ownership.protects(1, 0).is_none());
        assert!(ownership.take(10).is_empty());
    }

    #[test]
    fn bulk_cleanup_retires_both_generations_without_using_the_mutable_bank() {
        let mut ownership = Ownership::default();
        let first = ownership.begin(owner(10, 220, 2)).0;
        create(&mut ownership, first, 1, 101);
        create(&mut ownership, first, 2, 102);
        ownership.finish(first);
        let second = ownership.begin(owner(20, 221, 1)).0;
        create(&mut ownership, second, 3, 103);
        ownership.finish(second);
        let retired = ownership.take_all();
        assert_eq!(retired.len(), 3);
        for texture in retired {
            assert!(
                ownership
                    .claim_release(texture.handle, 100 + texture.handle as usize, Some(texture))
                    .is_some()
            );
        }
        assert!(ownership.take_all().is_empty());
    }

    #[test]
    fn cleanup_keeps_pending_com_alive_until_the_outer_creator_returns() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(token, 1).unwrap();
        assert!(ownership.take(10).is_empty());
        assert!(ownership.take_all().is_empty());
        // The bridge may have published COM, but the outer creator still needs
        // to call GetDesc/LockRect. Retirement must keep guarding that interval.
        assert!(ownership.protects(1, 101).is_some());
        assert!(ownership.claim_release(1, 101, None).is_none());
        assert!(ownership.protects(1, u32::MAX as usize).is_some());
        ownership.finish(token);
        let CreationResult::Retired(completed) = ownership.created(ticket, 101) else {
            panic!("retired native creator must return its completed texture for release");
        };
        assert_eq!((completed.handle, completed.pointer), (1, 101));
        assert!(ownership.protects(1, 101).is_none());
        assert!(ownership.claim_release(1, 101, Some(completed)).is_some());
        assert!(ownership.pending(token, 2).is_none());
    }

    #[test]
    fn cleanup_before_the_first_texture_does_not_forget_the_construction() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        assert!(ownership.take_all().is_empty());
        ownership.reserve(1);
        let ticket = ownership.pending(token, 1).unwrap();
        assert!(ownership.claim_release(1, 101, None).is_none());
        let CreationResult::Retired(completed) = ownership.created(ticket, 101) else {
            panic!("construction retired before its first texture must return that texture");
        };
        assert!(ownership.claim_release(1, 101, Some(completed)).is_some());
        ownership.finish(token);
        assert!(ownership.pending(token, 2).is_none());
    }

    #[test]
    fn a_reused_resource_address_does_not_own_the_previous_late_creation() {
        let mut ownership = Ownership::default();
        let first = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(first, 1).unwrap();
        let (second, retired) = ownership.begin(owner(10, 220, 1));
        assert_ne!(first, second);
        assert!(retired.is_empty());
        create(&mut ownership, second, 2, 102);
        let CreationResult::Retired(old) = ownership.created(ticket, 101) else {
            panic!("the old resource generation must not publish into its replacement");
        };
        ownership.finish(first);
        ownership.finish(second);
        assert!(ownership.claim_release(1, 101, Some(old)).is_some());
        assert!(ownership.protects(1, 101).is_none());
        assert_eq!(ownership.protects(2, 102).unwrap().resource, 10);
        let retired = ownership.take(10);
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].handle, 2);
    }

    #[test]
    fn failed_retired_creation_clears_its_pending_protection() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(token, 1).unwrap();
        assert!(ownership.take(10).is_empty());
        assert_eq!(ownership.created(ticket, 0), CreationResult::Discarded);
        ownership.finish(token);
        assert!(ownership.protects(1, 0).is_none());
        assert!(ownership.pending(token, 2).is_none());
    }

    #[test]
    fn a_late_creation_cannot_capture_a_recycled_handle_generation() {
        let mut ownership = Ownership::default();
        let first = ownership.begin(owner(10, 220, 1)).0;
        ownership.reserve(1);
        let ticket = ownership.pending(first, 1).unwrap();
        assert!(ownership.take(10).is_empty());
        let second = ownership.begin(owner(20, 221, 1)).0;
        create(&mut ownership, second, 1, 101);
        assert_eq!(ownership.created(ticket, 101), CreationResult::Discarded);
        ownership.finish(first);
        assert_eq!(ownership.protects(1, 101).unwrap().resource, 20);
    }

    #[test]
    fn native_cleanup_and_snapshot_release_share_one_claim() {
        let mut ownership = Ownership::default();
        let token = ownership.begin(owner(10, 220, 1)).0;
        create(&mut ownership, token, 1, 101);
        ownership.finish(token);
        assert!(ownership.claim_release(1, 101, None).is_none());
        let retired = ownership.take(10)[0];
        let ordinary = ownership.claim_release(1, 101, None).unwrap();
        assert!(ownership.claim_release(1, 101, Some(retired)).is_none());
        ownership.finish_release(ordinary);
        let snapshot = ownership.claim_release(1, 101, Some(retired)).unwrap();
        assert!(ownership.claim_release(1, 101, None).is_none());
        ownership.finish_release(snapshot);
        assert!(ownership.claim_release(1, 0, Some(retired)).is_none());
        assert!(ownership.claim_release(1, 999, Some(retired)).is_none());
    }

    #[test]
    fn a_finished_old_release_cannot_clear_the_recycled_handles_new_claim() {
        let mut ownership = Ownership::default();
        ownership.reserve(1);
        let old = ownership.claim_release(1, 101, None).unwrap();
        // Native Release has cleared its registry slot, allowing a reservation,
        // while the old callback is still returning to its Rust wrapper.
        ownership.reserve(1);
        let new = ownership.claim_release(1, 101, None).unwrap();
        assert_ne!(old, new);
        ownership.finish_release(old);
        assert!(ownership.claim_release(1, 101, None).is_none());
        ownership.finish_release(new);
        assert!(ownership.claim_release(1, 101, None).is_some());
    }
}
