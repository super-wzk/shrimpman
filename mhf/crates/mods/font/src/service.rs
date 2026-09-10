use crate::{FontApi, FontTable};
use safer_ffi::prelude::str;

/// A stable family-name table shared by the provider's lifecycle adapter.
pub(crate) struct FontService {
    pub(crate) api: FontTable,
}

struct FontState {
    family: String,
}

impl FontService {
    pub(crate) fn new(family: String) -> Self {
        Self {
            api: Box::new(FontState { family }).into(),
        }
    }
}

impl FontApi for FontState {
    fn family(&self) -> str::Ref<'_> {
        self.family.as_str().into()
    }
}
