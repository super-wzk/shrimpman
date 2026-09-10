//! Public-interface fixtures for native resource tests. Dictionary ordinals stay
//! private to the Translation provider.
use mhf_mod_sdk::abi;
use mhf_translation::{
    Key, MissingTranslation, TranslationApi, TranslationConfig, TranslationService,
    TranslationTable,
};
use safer_ffi::slice;

pub(super) enum TestTranslation {
    Dictionary(TranslationService),
    Fixture(TranslationTable),
}

impl TestTranslation {
    pub(super) fn with_locale(locale: Option<&str>, missing: MissingTranslation) -> Self {
        match locale {
            Some("test") | None => Self::Fixture(
                Box::new(Fixture {
                    translated_stage: locale.is_some(),
                    missing,
                })
                .into(),
            ),
            Some(locale) => Self::Dictionary(
                TranslationService::new(Some(&TranslationConfig {
                    locale: locale.into(),
                    missing,
                }))
                .unwrap(),
            ),
        }
    }

    pub(super) fn api(&self) -> &TranslationTable {
        match self {
            Self::Dictionary(service) => service.api(),
            Self::Fixture(table) => table,
        }
    }
}

struct Fixture {
    translated_stage: bool,
    missing: MissingTranslation,
}

impl TranslationApi for Fixture {
    fn resolve(
        &self,
        key: Key<'_>,
        mut buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> abi::Status {
        let text = if self.translated_stage && key == Key::stage(125, 23, 1) {
            "中文".to_owned()
        } else {
            match self.missing {
                MissingTranslation::Original => {
                    *required = 0;
                    return abi::NOT_FOUND;
                }
                MissingTranslation::Empty => String::new(),
                MissingTranslation::Key => format!("[{key}]"),
            }
        };
        *required = text.len();
        if buffer.len() < text.len() {
            return abi::BUFFER_TOO_SMALL;
        }
        buffer[..text.len()].copy_from_slice(text.as_bytes());
        abi::OK
    }
}
