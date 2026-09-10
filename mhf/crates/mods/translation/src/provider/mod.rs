//! Optional translated text and missing-translation policy.
//!
//! This module returns UTF-8 overrides. Native resource decoding, pointers and
//! allocation lifetimes belong to the text resource layer.

use crate::{Key, MissingTranslation, TranslationConfig};

mod dictionary;
mod service;
pub(crate) use dictionary::{CompiledDictionary, CompiledLocale, RuntimeLocale};
pub use service::TranslationService;
mod module;
pub use module::TranslationMod;
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/translations.rs"));

pub(crate) struct Translations {
    locale: Option<RuntimeLocale>,
    missing: MissingTranslation,
}

impl Translations {
    pub(crate) fn new(config: Option<&TranslationConfig>) -> Result<Self, String> {
        let locale = config
            .map(|config| {
                TRANSLATION_DICTIONARY.locale(&config.locale).ok_or_else(|| {
                    let available = TRANSLATION_DICTIONARY.locale_ids().collect::<Vec<_>>().join(", ");
                    format!(
                        "translation locale {:?} is not embedded; available locales: {available}",
                        config.locale
                    )
                })
            })
            .transpose()?;
        Ok(Self {
            locale,
            missing: config.map_or(MissingTranslation::Original, |config| config.missing),
        })
    }

    /// Return a NUL-terminated UTF-8 override, or leave source handling to the
    /// resource layer. Borrowed records remain valid for the process lifetime.
    pub(crate) fn resolve(&self, key: Key<'_>) -> Option<Cow<'static, [u8]>> {
        if let Some(translation) = self
            .locale
            .as_ref()
            .and_then(|locale| locale.translation(key))
        {
            return Some(Cow::Borrowed(translation));
        }
        self.missing(key)
    }

    fn missing(&self, key: Key<'_>) -> Option<Cow<'static, [u8]>> {
        match self.missing {
            MissingTranslation::Original => None,
            MissingTranslation::Empty => Some(Cow::Borrowed(b"\0")),
            MissingTranslation::Key => Some(Cow::Owned(format!("[{key}]\0").into_bytes())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MissingTranslation, TranslationConfig, Translations};

    #[test]
    fn unavailable_locale_is_rejected_before_resources_are_patched() {
        let config = TranslationConfig {
            locale: "not-embedded".into(),
            missing: MissingTranslation::Original,
        };
        assert!(Translations::new(Some(&config)).is_err());
    }
}
