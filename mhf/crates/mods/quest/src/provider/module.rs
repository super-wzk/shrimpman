use super::{QuestService, State, install};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module, Result};
use std::rc::Rc;
use windows::Win32::Foundation::HMODULE;

#[derive(Default)]
pub struct QuestMod {
    service: Rc<QuestService>,
    hook: Option<HookGuard<State>>,
}

impl QuestMod {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for QuestMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        unsafe {
            context.register(
                crate::api::INTERFACE_ID,
                (self.service.api() as *const crate::api::QuestTable).cast(),
            )?;
            context.register(
                crate::api::CONTROL_INTERFACE_ID,
                (self.service.control_api() as *const crate::api::QuestControlTable).cast(),
            )?;
            context.register(
                crate::api::LAUNCH_INTERFACE_ID,
                (self.service.launch_api() as *const crate::api::QuestLaunchTable).cast(),
            )
        }
    }

    fn attach(&mut self, context: &Context) -> Result<()> {
        if let Some(session) = self.service.session_for_attach() {
            self.hook = Some(unsafe { install(HMODULE(context.game().module_base), session) }?);
        }
        Ok(())
    }

    fn stop(&mut self, _context: &Context) -> Result<()> {
        self.service.seal();
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        if let Some(state) = self.hook.as_mut().and_then(HookGuard::retired_state_mut) {
            unsafe { state.prepare_release() }?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QuestApi, QuestLaunchApi};
    use mhf_mod_host::{ModHost, api};
    use mhf_mod_package::{Candidate, Kind, Manifest, Resolved, Version};
    use std::collections::BTreeMap;

    #[test]
    fn idle_attachment_does_not_require_a_game_or_install_native_hooks() {
        let candidate = Candidate::builtin(Manifest {
            schema: 1,
            id: crate::PROVIDER_ID.into(),
            name: "Idle task provider".into(),
            version: Version::new(1, 0, 0),
            kind: Kind::Native,
            entry: None,
            dependencies: BTreeMap::new(),
        })
        .unwrap();
        let mut quest = Some(QuestMod::new());
        let service = quest.as_ref().unwrap().service.clone();
        let mut host = ModHost::load(
            Resolved {
                mods: vec![candidate],
            },
            &BTreeMap::new(),
            |_| Ok(Box::new(quest.take().unwrap())),
        )
        .unwrap();
        host.prepare().unwrap();
        host.check().unwrap();
        // No game base is published. An attempted native install would fail;
        // the idle provider must complete the real attach callback without it.
        host.attach().unwrap();
        assert_eq!(service.api().snapshot(), crate::Snapshot::default());
        assert_eq!(
            service.launch_api().prepare_local((&[][..]).into()),
            api::ERROR
        );
        host.stop().unwrap();
        unsafe { host.detach(&[]) }.unwrap();
        unsafe { host.prepare_release() }.unwrap();
    }
}
