use crate::{FontApi, FontTable};
use safer_ffi::prelude::str;
use std::sync::{Arc, OnceLock};

/// A stable family-name table shared by the provider's lifecycle adapter.
pub(crate) struct FontService {
    pub(crate) api: FontTable,
    launch_family: Arc<OnceLock<String>>,
}

struct FontState {
    family: String,
    launch_family: Arc<OnceLock<String>>,
}

impl FontService {
    pub(crate) fn new(family: String) -> Self {
        let launch_family = Arc::new(OnceLock::new());
        Self {
            api: Box::new(FontState {
                family,
                launch_family: launch_family.clone(),
            })
            .into(),
            launch_family,
        }
    }

    pub(crate) fn set_family(&self, family: String) -> Result<(), String> {
        self.launch_family
            .set(family)
            .map_err(|_| "launch font family is already finalized".to_owned())
    }
}

impl FontApi for FontState {
    fn family(&self) -> str::Ref<'_> {
        self.launch_family
            .get()
            .unwrap_or(&self.family)
            .as_str()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_launch_family_keeps_earlier_borrows_valid() {
        let service = FontService::new("Before".into());
        let original = service.api.family();
        service.set_family("After".into()).unwrap();
        assert_eq!(original.as_str(), "Before");
        assert_eq!(service.api.family().as_str(), "After");
    }
}
