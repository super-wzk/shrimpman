//! A font family's Rust interface and generated C table.

use mhf_mod_sdk::{
    Dependencies, Result,
    interface::{Interface, InterfaceRef, bind},
};
use safer_ffi::prelude::{VirtualPtr, derive_ReprC, str};

pub const PROVIDER_ID: &str = "mhf.base";
pub const INTERFACE_ID: &str = "mhf.font.v1";
pub const FAMILY_NAME: &str = "JetBrains Maple Mono NF NL HT";

/// An immutable family name, readable concurrently without allocating.
/// Implementations must return valid UTF-8 borrowed from their own stable
/// storage and must not unwind through the generated C entry point.
#[derive_ReprC(dyn)]
pub trait FontApi: Send + Sync {
    /// The returned bytes remain valid for the object borrow. C consumers must
    /// not write or free them, or retain them beyond the provider's lifetime.
    fn family(&self) -> str::Ref<'_>;
}

/// A host lookup lends this generated object through consumer destruction.
/// The provider owns it: never consume or free it, copy it into another owner,
/// or call its C `vtable.release_vptr`. No pointer copy retains its DLL.
pub type FontTable = VirtualPtr<dyn FontApi + Send + Sync>;

pub enum FontInterface {}
// SAFETY: The ID identifies the generated FontApi table. Providers keep the
// immutable object and its borrowed name alive through consumer destruction.
unsafe impl Interface for FontInterface {
    type Table = FontTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

#[repr(transparent)]
pub struct Font<'host> {
    binding: InterfaceRef<'host, FontInterface>,
}
impl<'host> Font<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            binding: bind(dependencies)?,
        })
    }

    #[inline]
    pub fn family(&self) -> &str {
        self.binding.table().family().as_str()
    }
}

#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<FontTable>(definer, "FontTable")?;
    h::string(definer, "MHF_FONT_PROVIDER", PROVIDER_ID)?;
    h::string(definer, "MHF_FONT_INTERFACE", INTERFACE_ID)
}
