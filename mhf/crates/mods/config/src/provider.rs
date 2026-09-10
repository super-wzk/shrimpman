use crate::{ConfigApi, ConfigTable, Registration, Store};
use mhf_mod_sdk::abi;
use safer_ffi::{
    prelude::{VirtualPtr, str},
    slice,
};
use std::sync::{Arc, Mutex};

pub struct ConfigService {
    table: ConfigTable,
}
impl ConfigService {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            table: VirtualPtr::from(Box::new(Service {
                store,
                error: Mutex::new(String::new()),
            })),
        }
    }
    pub fn api(&self) -> &ConfigTable {
        &self.table
    }
}
struct Service {
    store: Arc<Mutex<Store>>,
    error: Mutex<String>,
}
impl Service {
    fn result(&self, result: Result<(), String>) -> abi::Status {
        match result {
            Ok(()) => abi::OK,
            Err(error) => {
                *self
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = error;
                abi::ERROR
            }
        }
    }
}
fn copy(bytes: &[u8], mut buffer: slice::Mut<'_, u8>, required: &mut usize) -> abi::Status {
    *required = bytes.len();
    if buffer.len() < bytes.len() {
        return abi::BUFFER_TOO_SMALL;
    }
    buffer[..bytes.len()].copy_from_slice(bytes);
    abi::OK
}
impl ConfigApi for Service {
    fn register_section(&self, section: str::Ref<'_>, definition: str::Ref<'_>) -> abi::Status {
        self.result((|| {
            let definition: Registration =
                toml::from_str(definition.as_str()).map_err(|error| error.to_string())?;
            self.store
                .lock()
                .map_err(|_| "configuration lock poisoned")?
                .register(section.as_str(), definition)
        })())
    }
    fn read(
        &self,
        section: str::Ref<'_>,
        buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> abi::Status {
        let result = (|| {
            let table = self
                .store
                .lock()
                .map_err(|_| "configuration lock poisoned")?
                .read(section.as_str())?;
            toml::to_string(&table).map_err(|error| error.to_string())
        })();
        match result {
            Ok(text) => copy(text.as_bytes(), buffer, required),
            Err(error) => self.result(Err(error)),
        }
    }
    fn write(&self, section: str::Ref<'_>, patch: str::Ref<'_>) -> abi::Status {
        self.result((|| {
            let patch = toml::from_str(patch.as_str()).map_err(|error| error.to_string())?;
            self.store
                .lock()
                .map_err(|_| "configuration lock poisoned")?
                .write(section.as_str(), patch)
        })())
    }
    fn last_error(&self, buffer: slice::Mut<'_, u8>, required: &mut usize) -> abi::Status {
        let error = self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        copy(error.as_bytes(), buffer, required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_table_registers_reads_and_writes_a_consumers_configuration() {
        let path = std::env::temp_dir().join(format!("mhf-config-api-{}.toml", std::process::id()));
        std::fs::write(&path, "[client]\nlabel = '猎人'\n[other]\nvalue = 7\n").unwrap();
        let store = Arc::new(Mutex::new(Store::load(path.clone()).unwrap()));
        let service = ConfigService::new(store);
        // The local service owns its object for the entire consumer borrow.
        let config = unsafe { crate::bind(service.api()) };
        config
            .register(
                "client",
                &Registration {
                    defaults: toml::from_str("label = ''\nenabled = true").unwrap(),
                    ..Default::default()
                },
            )
            .unwrap();
        let values: toml::Table = toml::from_str(&config.read("client").unwrap()).unwrap();
        assert_eq!(values["label"].as_str(), Some("猎人"));
        assert_eq!(values["enabled"].as_bool(), Some(true));
        config
            .write("client", "label = '测试'\nenabled = false")
            .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let document: toml::Table = toml::from_str(&before).unwrap();
        assert_eq!(document["other"]["value"].as_integer(), Some(7));
        assert_eq!(document["client"]["label"].as_str(), Some("测试"));
        assert!(config.write("client", "enabled = 42").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }
}
