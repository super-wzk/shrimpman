//! Borrowed translation keys and provider operations, shared by Rust and C.
use mhf_mod_sdk::{
    Dependencies, Error, Result, abi as api,
    error::status_result,
    interface::{Interface, bind as bind_interface},
};
use safer_ffi::{
    prelude::{VirtualPtr, derive_ReprC, str},
    slice,
};
use std::fmt;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TranslationConfig {
    pub locale: String,
    #[serde(default)]
    pub missing: MissingTranslation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissingTranslation {
    #[default]
    Original,
    Key,
    Empty,
}

pub const PROVIDER_ID: &str = "mhf.translation";
pub const INTERFACE_ID: &str = "mhf.translation.v1";

#[derive_ReprC(rename = "TranslationKeyKind")]
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyKind {
    Resource = 1,
    Stage = 2,
}

/// A borrowed key containing stable resource identifiers, never catalog ordinals.
/// Fields for the other kind are ignored. C callers must provide valid UTF-8
/// string slices with non-null pointers, including empty slices; all borrowed
/// bytes must remain immutable and live for the resolve call.
#[derive_ReprC(rename = "TranslationKey")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Key<'a> {
    kind: KeyKind,
    resource_id: str::Ref<'a>,
    group_id: str::Ref<'a>,
    record_id: u32,
    part: u16,
    stage_id: u16,
    section: u16,
    record: u16,
}

impl<'a> Key<'a> {
    #[inline]
    pub fn resource(resource_id: &'a str, group_id: &'a str, record_id: u32, part: u16) -> Self {
        Self {
            kind: KeyKind::Resource,
            resource_id: resource_id.into(),
            group_id: group_id.into(),
            record_id,
            part,
            stage_id: 0,
            section: 0,
            record: 0,
        }
    }

    #[inline]
    pub fn stage(stage_id: u16, section: u16, record: u16) -> Self {
        Self {
            kind: KeyKind::Stage,
            resource_id: "".into(),
            group_id: "".into(),
            record_id: 0,
            part: 0,
            stage_id,
            section,
            record,
        }
    }

    #[inline]
    pub fn kind(self) -> KeyKind {
        self.kind
    }

    #[inline]
    pub fn resource_id(self) -> &'a str {
        self.resource_id.as_str()
    }

    #[inline]
    pub fn group_id(self) -> &'a str {
        self.group_id.as_str()
    }

    #[inline]
    pub fn record_id(self) -> u32 {
        self.record_id
    }

    #[inline]
    pub fn part(self) -> u16 {
        self.part
    }

    #[inline]
    pub fn stage_id(self) -> u16 {
        self.stage_id
    }

    #[inline]
    pub fn section(self) -> u16 {
        self.section
    }

    #[inline]
    pub fn record(self) -> u16 {
        self.record
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && match self.kind {
                KeyKind::Resource => {
                    self.resource_id() == other.resource_id()
                        && self.group_id() == other.group_id()
                        && self.record_id == other.record_id
                        && self.part == other.part
                }
                KeyKind::Stage => {
                    self.stage_id == other.stage_id
                        && self.section == other.section
                        && self.record == other.record
                }
            }
    }
}
impl Eq for Key<'_> {}

impl fmt::Display for Key<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            KeyKind::Resource => {
                write!(
                    formatter,
                    "{}:{}:{}",
                    self.resource_id(),
                    self.group_id(),
                    self.record_id
                )?;
                if self.part != 0 {
                    write!(formatter, ":{:02}", self.part)?;
                }
                Ok(())
            }
            KeyKind::Stage => write!(
                formatter,
                "stage:{:03}:{:04X}:{:04X}",
                self.stage_id, self.section, self.record
            ),
        }
    }
}

/// An immutable provider, callable concurrently from native resource threads.
/// Implementations must retain no caller pointers and must not unwind through
/// a generated C entry point. A key's result stays stable between a capacity
/// probe and a later read.
#[derive_ReprC(dyn)]
pub trait TranslationApi: Send + Sync {
    /// Copies UTF-8 bytes without a terminating NUL. `required` counts bytes.
    /// `BUFFER_TOO_SMALL` writes that count and no partial translation; `OK`
    /// writes exactly that many bytes. `NOT_FOUND` requests original source
    /// handling; `OK` with `required = 0` explicitly replaces it with empty text.
    ///
    /// C callers must provide a writable, non-null `required` pointer and a
    /// valid mutable slice, whose pointer is non-null even for a size probe.
    /// Outputs must not overlap each other, the key's string bytes, provider
    /// storage, or another active access. Keep all borrows and provider code live
    /// for the call; the generated signature preserves these borrows for Rust.
    fn resolve(
        &self,
        key: Key<'_>,
        buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> api::Status;
}

/// The provider owns this virtual object. A host lookup lends an immutable table
/// through consumer destruction and transfers no ownership. Consumers must not
/// move, free, overwrite, or adopt a copy of it as an owner, nor call
/// `vtable.release_vptr`. A pointer copy or reference count does not retain the
/// provider DLL; all uses must end before the host releases it.
pub type TranslationTable = VirtualPtr<dyn TranslationApi + Send + Sync>;

pub enum TranslationInterface {}

// SAFETY: This ID fixes the generated table and requires immutable concurrent
// resolution, with provider storage and code valid through consumer destruction.
unsafe impl Interface for TranslationInterface {
    type Table = TranslationTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Translation<'provider> {
    table: &'provider TranslationTable,
}

impl<'provider> Translation<'provider> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'provider>) -> Result<Self> {
        Ok(Self {
            table: bind_interface::<TranslationInterface>(dependencies)?.table(),
        })
    }

    pub fn resolve(&self, key: Key<'_>) -> Result<Option<String>> {
        let mut required = 0;
        let status = self.table.resolve(key, (&mut [][..]).into(), &mut required);
        if status == api::NOT_FOUND {
            return Ok(None);
        }
        if status != api::BUFFER_TOO_SMALL {
            status_result(status, "resolve translation")?;
        }
        if required == 0 {
            return Ok(Some(String::new()));
        }
        let mut text = vec![0; required];
        let status = self
            .table
            .resolve(key, text.as_mut_slice().into(), &mut required);
        status_result(status, "resolve translation")?;
        text.truncate(required);
        String::from_utf8(text)
            .map(Some)
            .map_err(|error| Error::new(format!("translation is not UTF-8: {error}")))
    }
}

/// Binds an immutable provider table retained by a native host state.
///
/// # Safety
/// `table` must be non-null, aligned, and point to an initialized
/// `TranslationTable` implementing this interface. The table, object, vtable,
/// and provider code must remain valid for `'provider`, including concurrent
/// calls. This borrows without acquiring ownership or retaining the DLL; native
/// consumers must stop using it before provider release.
#[inline]
pub unsafe fn bind<'provider>(table: *const TranslationTable) -> Translation<'provider> {
    Translation {
        table: unsafe { &*table },
    }
}

#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<Key<'static>>(definer, "TranslationKey")?;
    h::alias::<TranslationTable>(definer, "TranslationTable")?;
    h::string(definer, "MHF_TRANSLATION_PROVIDER_ID", PROVIDER_ID)?;
    h::string(definer, "MHF_TRANSLATION_INTERFACE_ID", INTERFACE_ID)?;
    Ok(())
}

#[cfg(test)]
mod tests;
