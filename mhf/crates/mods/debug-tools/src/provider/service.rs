use super::DebugControl;
use crate::api::{Command, DebugApi, DebugTable, Snapshot};
use mhf_mod_sdk::abi as api;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

pub struct DebugService {
    api: DebugTable,
}

struct DebugState {
    control: Arc<DebugControl>,
}

impl DebugService {
    pub fn new(control: Arc<DebugControl>) -> Self {
        Self {
            api: Box::new(DebugState { control }).into(),
        }
    }

    pub fn api(&self) -> &DebugTable {
        &self.api
    }
}

impl DebugApi for DebugState {
    fn snapshot(&self) -> Snapshot {
        catch_unwind(AssertUnwindSafe(|| self.control.mod_snapshot())).unwrap_or_default()
    }
    fn transform(&self, species: u8) -> api::Status {
        self.submit(Command::Transform(species))
    }
    fn restore_hunter(&self) -> api::Status {
        self.submit(Command::RestoreHunter)
    }
    fn change_area(&self, area: u16) -> api::Status {
        self.submit(Command::ChangeArea(area))
    }
    fn restart(&self) -> api::Status {
        self.submit(Command::Restart)
    }
    fn exit(&self) -> api::Status {
        self.submit(Command::Exit)
    }
}

impl DebugState {
    fn submit(&self, command: Command) -> api::Status {
        catch_unwind(AssertUnwindSafe(|| {
            match self.control.mod_command(command) {
                Ok(()) => api::OK,
                Err(error) => {
                    eprintln!("mhf.debug-tools: {error}");
                    api::ERROR
                }
            }
        }))
        .unwrap_or(api::ERROR)
    }
}
