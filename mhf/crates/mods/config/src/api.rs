//! Configuration registration and access without consumer domain types.

use mhf_mod_sdk::{
    Dependencies, Error, Result, abi,
    interface::{Interface, bind as bind_interface},
};
use safer_ffi::{
    prelude::{VirtualPtr, derive_ReprC, str},
    slice,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROVIDER_ID: &str = "mhf.config";
pub const INTERFACE_ID: &str = "mhf.config.v1";

/// Consumer-owned defaults apply in memory without rewriting the user's file.
/// Fixed values apply last, both when reading and when persisting a change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Registration {
    pub defaults: toml::Table,
    pub fixed: toml::Table,
    pub ini: Option<IniSection>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IniSection {
    pub name: String,
    pub fields: Vec<IniField>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IniField {
    pub key: String,
    pub path: Vec<String>,
    pub kind: IniKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IniKind {
    Boolean,
    Integer { min: i64, max: i64 },
    String,
    Enum { values: BTreeMap<String, i64> },
}

/// Concurrent calls copy all input and output bytes. Definitions and values
/// are UTF-8 TOML. Duplicate registration requires identical definitions.
/// Implementations must not unwind or retain caller pointers across a call.
#[derive_ReprC(dyn)]
pub trait ConfigApi: Send + Sync {
    fn register_section(&self, section: str::Ref<'_>, definition: str::Ref<'_>) -> abi::Status;
    /// Copies TOML without a NUL terminator. BUFFER_TOO_SMALL sets required
    /// and writes no partial output. Retry if a concurrent write grows it.
    fn read(
        &self,
        section: str::Ref<'_>,
        buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> abi::Status;
    /// Merge a patch into the latest file, preserving untouched fields.
    /// Failed validation leaves the file intact.
    fn write(&self, section: str::Ref<'_>, patch: str::Ref<'_>) -> abi::Status;
    /// Last failure diagnostic; another concurrent failure may replace it.
    fn last_error(&self, buffer: slice::Mut<'_, u8>, required: &mut usize) -> abi::Status;
}

/// Borrowed provider-owned object. Never release or adopt a table returned by
/// host lookup. Calls must end before provider destruction; a copied borrow
/// does not retain the provider DLL.
pub type ConfigTable = VirtualPtr<dyn ConfigApi + Send + Sync>;
pub enum ConfigInterface {}
// SAFETY: the interface fixes this generated table and its provider lifetime.
unsafe impl Interface for ConfigInterface {
    type Table = ConfigTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

#[derive(Clone, Copy)]
pub struct Config<'provider> {
    table: &'provider ConfigTable,
}
impl<'provider> Config<'provider> {
    pub fn bind(dependencies: Dependencies<'provider>) -> Result<Self> {
        Ok(Self {
            table: bind_interface::<ConfigInterface>(dependencies)?.table(),
        })
    }
    pub fn register(&self, section: &str, definition: &Registration) -> Result<()> {
        let definition =
            toml::to_string(definition).map_err(|error| Error::new(error.to_string()))?;
        self.check(
            self.table
                .register_section(section.into(), definition.as_str().into()),
            "register configuration",
        )
    }
    pub fn read(&self, section: &str) -> Result<String> {
        read_text(|buffer, required| self.table.read(section.into(), buffer, required))
            .map_err(|error| self.failure(&error.to_string()))
    }
    pub fn write(&self, section: &str, patch: &str) -> Result<()> {
        self.check(
            self.table.write(section.into(), patch.into()),
            "write configuration",
        )
    }
    fn check(&self, status: abi::Status, operation: &str) -> Result<()> {
        if status == abi::OK {
            Ok(())
        } else {
            Err(self.failure(operation))
        }
    }
    fn failure(&self, operation: &str) -> Error {
        let detail = read_text(|buffer, required| self.table.last_error(buffer, required))
            .unwrap_or_default();
        Error::new(format!("{operation}: {detail}"))
    }
}
fn read_text(
    mut read: impl FnMut(slice::Mut<'_, u8>, &mut usize) -> abi::Status,
) -> Result<String> {
    let mut bytes = Vec::new();
    for _ in 0..4 {
        let mut required = 0;
        let status = read(bytes.as_mut_slice().into(), &mut required);
        match status {
            abi::OK if required <= bytes.len() => {
                bytes.truncate(required);
                return String::from_utf8(bytes).map_err(|error| Error::new(error.to_string()));
            }
            abi::BUFFER_TOO_SMALL => bytes.resize(required, 0),
            _ => {
                return Err(Error::new(format!(
                    "configuration read failed with status {status}"
                )));
            }
        }
    }
    Err(Error::new("configuration changed repeatedly while reading"))
}
/// Borrow a table retained by the host's dependency graph.
///
/// # Safety
/// The initialized table, object, vtable and provider code must remain valid
/// for 'provider. All consumer calls must end before provider release.
pub unsafe fn bind<'provider>(table: *const ConfigTable) -> Config<'provider> {
    Config {
        table: unsafe { &*table },
    }
}
#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<ConfigTable>(definer, "ConfigTable")?;
    h::string(definer, "MHF_CONFIG_PROVIDER", PROVIDER_ID)?;
    h::string(definer, "MHF_CONFIG_INTERFACE", INTERFACE_ID)
}
