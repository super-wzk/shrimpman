use super::{TranslationConfig, Translations};
use crate::api::{Key, TranslationApi, TranslationTable};
use mhf_mod_sdk::abi as api;
use safer_ffi::slice;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The table owns the immutable provider state through all consumers' lifetimes.
pub struct TranslationService {
    api: TranslationTable,
}

impl TranslationService {
    pub fn new(config: Option<&TranslationConfig>) -> Result<Self, String> {
        Ok(Self {
            api: Box::new(Translations::new(config)?).into(),
        })
    }
    pub fn api(&self) -> &TranslationTable {
        &self.api
    }
}
impl TranslationApi for Translations {
    fn resolve(
        &self,
        key: Key<'_>,
        mut buffer: slice::Mut<'_, u8>,
        required: &mut usize,
    ) -> api::Status {
        *required = 0;
        catch_unwind(AssertUnwindSafe(|| {
            let Some(text) = self.resolve(key) else {
                return api::NOT_FOUND;
            };
            // Compiled records and all missing-policy results have one final NUL.
            let text = &text[..text.len() - 1];
            *required = text.len();
            if buffer.len() < text.len() {
                return api::BUFFER_TOO_SMALL;
            }
            buffer[..text.len()].copy_from_slice(text);
            api::OK
        }))
        .unwrap_or(api::ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MissingTranslation;

    fn provider(missing: MissingTranslation) -> TranslationService {
        TranslationService {
            api: Box::new(Translations {
                locale: None,
                missing,
            })
            .into(),
        }
    }

    #[test]
    fn c_provider_copies_without_nul_and_retains_missing_policy() {
        let resource = String::from("mhfdat");
        let group = String::from("table");
        let key = Key::resource(&resource, &group, 42, 1);
        for (missing, expected) in [
            (MissingTranslation::Original, None),
            (MissingTranslation::Empty, Some("")),
            (MissingTranslation::Key, Some("[mhfdat:table:42:01]")),
        ] {
            let provider = provider(missing);
            let binding = unsafe { crate::api::bind(provider.api()) };
            assert_eq!(binding.resolve(key).unwrap().as_deref(), expected);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| assert_eq!(binding.resolve(key).unwrap().as_deref(), expected));
                }
            });
        }
    }

    #[test]
    fn resource_ids_resolve_in_the_providers_catalog() {
        let config = TranslationConfig {
            locale: "ja-JP".into(),
            missing: MissingTranslation::Original,
        };
        let provider = TranslationService::new(Some(&config)).unwrap();
        let binding = unsafe { crate::api::bind(provider.api()) };
        let resource = String::from("mhfdat");
        let group = String::from("head_armor_names");
        let key = Key::resource(&resource, &group, 0, 0);
        let mut output = [0; 64];
        let mut required = 0;
        assert_eq!(
            provider
                .api
                .resolve(key, output.as_mut_slice().into(), &mut required),
            api::OK
        );
        assert_eq!(&output[..required], "装備無し".as_bytes());
        let missing = Key::resource(&resource, "unknown_group", 0, 0);
        assert_eq!(binding.resolve(missing).unwrap(), None);
    }

    #[test]
    fn direct_c_calls_probe_size_without_partial_writes_or_trailing_nul() {
        let provider = provider(MissingTranslation::Key);
        let key = Key::stage(1, 23, 42);
        let api = provider.api();
        let mut required = 0;
        assert_eq!(
            api.resolve(key, (&mut [][..]).into(), &mut required),
            mhf_mod_sdk::abi::BUFFER_TOO_SMALL
        );
        let length = required;
        let mut text = vec![0xAA; length + 1];
        assert_eq!(
            api.resolve(key, (&mut text[..length - 1]).into(), &mut required),
            mhf_mod_sdk::abi::BUFFER_TOO_SMALL
        );
        assert!(text.iter().all(|byte| *byte == 0xAA));
        assert_eq!(
            api.resolve(key, (&mut text[..length]).into(), &mut required),
            mhf_mod_sdk::abi::OK
        );
        assert_eq!(&text[..length], b"[stage:001:0017:002A]");
        assert_eq!(text[length], 0xAA);
    }
}
