//! Optional translated text and missing-translation policy.
//!
//! This module returns UTF-8 overrides. Native resource decoding, pointers and
//! allocation lifetimes belong to the text resource layer.

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
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

#[cfg(all(feature = "offline", feature = "translation"))]
pub(crate) mod offline_quest;

#[cfg(feature = "unicode")]
mod key;
#[cfg(feature = "unicode")]
pub(crate) use key::TranslationKey;

#[cfg(feature = "translation")]
mod dictionary;
#[cfg(feature = "translation")]
pub(crate) use dictionary::{CompiledDictionary, CompiledLocale, RuntimeLocale};
#[cfg(feature = "translation")]
use std::borrow::Cow;

#[cfg(feature = "translation")]
include!(concat!(env!("OUT_DIR"), "/translations.rs"));

#[cfg(feature = "translation")]
#[derive(Default)]
pub(crate) struct Translations {
    locale: Option<RuntimeLocale>,
    missing: MissingTranslation,
}

#[cfg(feature = "translation")]
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
    pub(crate) fn resolve(&self, key: TranslationKey) -> Option<Cow<'static, [u8]>> {
        if let Some(translation) = self
            .locale
            .as_ref()
            .and_then(|locale| locale.translation(key))
        {
            return Some(Cow::Borrowed(translation));
        }
        match self.missing {
            MissingTranslation::Original => None,
            MissingTranslation::Empty => Some(Cow::Borrowed(b"\0")),
            MissingTranslation::Key => Some(Cow::Owned(format!("[{key}]\0").into_bytes())),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_locale(locale: Option<RuntimeLocale>, missing: MissingTranslation) -> Self {
        Self { locale, missing }
    }
}

#[cfg(all(test, feature = "translation"))]
pub(crate) fn test_locale(id: &str) -> RuntimeLocale {
    TRANSLATION_DICTIONARY
        .locale(id)
        .expect("embedded test locale")
}

#[cfg(all(test, feature = "translation"))]
mod tests {
    use super::{MissingTranslation, TranslationConfig, TranslationKey, Translations};

    #[test]
    fn disabled_translation_leaves_original_text_to_the_resource_layer() {
        let key = TranslationKey::Stage {
            stage: 1,
            section: 23,
            record: 2,
        };
        assert!(Translations::new(None).unwrap().resolve(key).is_none());
        assert_eq!(
            Translations::with_locale(None, MissingTranslation::Key)
                .resolve(key)
                .unwrap()
                .as_ref(),
            b"[stage:001:0017:0002]\0"
        );
        assert_eq!(
            Translations::with_locale(None, MissingTranslation::Empty)
                .resolve(key)
                .unwrap()
                .as_ref(),
            b"\0"
        );
    }

    #[test]
    fn unavailable_locale_is_rejected_before_resources_are_patched() {
        let config = TranslationConfig {
            locale: "not-embedded".into(),
            missing: MissingTranslation::Original,
        };
        assert!(Translations::new(Some(&config)).is_err());
    }
}
