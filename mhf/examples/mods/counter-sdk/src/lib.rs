//! One typed counter contract, used directly by Rust and generated into C.

use mhf_mod_sdk::{
    Dependencies, Result, abi as api,
    interface::{Interface, InterfaceRef, bind},
};
use safer_ffi::prelude::{VirtualPtr, derive_ReprC};

pub const PROVIDER_ID: &str = "example.counter";
pub const INTERFACE_ID: &str = "example.counter.v1";

#[derive_ReprC(rename = "CounterSnapshot")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub count: u32,
}

/// A synchronized counter. Implementations may run concurrently and must never
/// unwind through generated C entry points. Snapshot is an infallible value
/// return; addition reports overflow without changing the stored count.
#[derive_ReprC(dyn)]
pub trait CounterApi: Send + Sync {
    fn snapshot(&self) -> Snapshot;
    fn add(&self, amount: u32) -> api::Status;
}

/// The sole interface table. The provider owns this virtual object and keeps it
/// in stable shared storage. A dependency lookup lends the table until consumer
/// destruction; it does not transfer ownership or keep the provider DLL loaded.
/// Consumers must not adopt or byte-copy this table as an owner, overwrite it,
/// or call vtable.release_vptr. All calls end before the provider is released.
pub type CounterTable = VirtualPtr<dyn CounterApi + Send + Sync>;

pub enum CounterInterface {}

// SAFETY: This ID identifies the generated CounterApi table and provider-owned
// lifetime. The provider permits concurrent calls to the borrowed object.
unsafe impl Interface for CounterInterface {
    type Table = CounterTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

/// A borrowed binding with Result convenience for fallible additions. Snapshot
/// is the same strongly typed value returned by CounterApi, without a mirror.
#[repr(transparent)]
pub struct Counter<'host> {
    binding: InterfaceRef<'host, CounterInterface>,
}

impl<'host> Counter<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            binding: bind(dependencies)?,
        })
    }

    #[inline]
    pub fn snapshot(&self) -> Snapshot {
        self.binding.table().snapshot()
    }

    #[inline]
    pub fn add(&self, amount: u32) -> Result<()> {
        mhf_mod_sdk::error::status_result(self.binding.table().add(amount), "counter addition")
    }
}

#[cfg(feature = "headers")]
pub fn generate_headers(include_dir: &std::path::Path) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::generate(&include_dir.join("mhf_mod.h"))?;
    h::write(
        &include_dir.join("counter.h"),
        "EXAMPLE_COUNTER_H",
        Some("mhf_mod.h"),
        h::definitions()?,
        |d| {
            h::alias::<Snapshot>(d, "CounterSnapshot")?;
            h::alias::<CounterTable>(d, "CounterTable")?;
            h::string(d, "EXAMPLE_COUNTER_PROVIDER", PROVIDER_ID)?;
            h::string(d, "EXAMPLE_COUNTER_INTERFACE", INTERFACE_ID)
        },
    )
}
