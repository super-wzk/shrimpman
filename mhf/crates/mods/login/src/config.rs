use ::config::{Config as LayeredConfig, Environment, File, FileFormat};
use serde::Deserialize;

const SIGN_SECTION: &str = "sign";

pub(crate) fn load(section: &str) -> Result<SignSettings, String> {
    load_sign_settings(section, sign_environment())
        .map_err(|error| format!("failed to resolve [sign] configuration: {error}"))
}

pub(crate) fn endpoint_override() -> Option<String> {
    std::env::vars().find_map(|(key, value)| {
        key.eq_ignore_ascii_case("MHF_SIGN__ENDPOINT")
            .then_some(value)
    })
}

fn sign_environment() -> Environment {
    Environment::with_prefix("MHF")
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignEncoding {
    #[default]
    Utf8,
    ShiftJis,
}

impl SignEncoding {
    pub fn decode(self, bytes: &[u8]) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes),
            Self::ShiftJis => encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0,
        }
    }

    pub fn encode(self, text: &str) -> Result<std::borrow::Cow<'_, [u8]>, String> {
        if self == Self::Utf8 || text.is_empty() {
            return Ok(std::borrow::Cow::Borrowed(text.as_bytes()));
        }
        let (bytes, _, replaced) = encoding_rs::SHIFT_JIS.encode(text);
        // Typed credentials must still identify the same account after encoding.
        if replaced || self.decode(&bytes) != text {
            return Err(
                "The username or password cannot be represented exactly in Shift-JIS".into(),
            );
        }
        Ok(bytes)
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignSettings {
    pub endpoint: String,
    #[serde(default)]
    pub encoding: SignEncoding,
}

fn load_sign_settings(section: &str, environment: Environment) -> Result<SignSettings, String> {
    let section: toml::Table = toml::from_str(section).map_err(|error| error.to_string())?;
    let document = toml::Table::from_iter([(SIGN_SECTION.into(), toml::Value::Table(section))]);
    let source = toml::to_string(&document).map_err(|error| error.to_string())?;
    LayeredConfig::builder()
        .add_source(File::from_str(&source, FileFormat::Toml))
        .add_source(environment)
        .build()
        .and_then(|config| config.get::<SignSettings>(SIGN_SECTION))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SOURCE: &str = "endpoint = \"http://127.0.0.1:53313\"\n";

    fn document() -> toml::Table {
        toml::from_str(SOURCE).expect("test config should parse")
    }

    #[test]
    fn environment_overrides_only_the_sign_configuration() {
        let environment = sign_environment().source(Some(HashMap::from([
            (
                "MHF_SIGN__ENDPOINT".to_owned(),
                "http://127.0.0.1:60000".to_owned(),
            ),
            ("MHF_WINE".to_owned(), "winecx24".to_owned()),
        ])));

        let settings =
            load_sign_settings(SOURCE, environment).expect("sign configuration should load");

        assert_eq!(settings.endpoint, "http://127.0.0.1:60000");
        assert_eq!(settings.encoding, SignEncoding::Utf8);
        assert_eq!(
            document()["endpoint"].as_str(),
            Some("http://127.0.0.1:53313")
        );
    }

    #[test]
    fn tcp_endpoint_can_be_selected_by_config_or_environment() {
        let environment = sign_environment().source(Some(HashMap::from([(
            "MHF_SIGN__ENDPOINT".into(),
            "tcp://localhost:60001".into(),
        )])));
        assert_eq!(
            load_sign_settings(SOURCE, environment).unwrap().endpoint,
            "tcp://localhost:60001"
        );
        let environment = sign_environment().source(Some(HashMap::new()));
        assert_eq!(
            load_sign_settings("endpoint = 'tcp://[::1]:53000'", environment)
                .unwrap()
                .endpoint,
            "tcp://[::1]:53000",
        );
    }

    #[test]
    fn sign_configuration_requires_one_endpoint() {
        for source in [
            "",
            "endpoint = 'tcp://localhost:53000'\ntransport = 'tcp'",
            "[http]\nbase_url = 'http://localhost:53001'",
        ] {
            let environment = sign_environment().source(Some(HashMap::new()));
            assert!(load_sign_settings(source, environment).is_err(), "{source}");
        }
    }

    #[test]
    fn online_launch_still_requires_valid_sign_settings() {
        let environment = sign_environment().source(Some(HashMap::new()));
        assert!(load_sign_settings("[screen]\nmode = \"windowed\"", environment).is_err());
    }

    #[test]
    fn nested_unknown_fields_stay_inside_the_sign_section() {
        let environment = sign_environment().source(Some(HashMap::new()));
        let error = load_sign_settings(
            "endpoint = 'tcp://localhost:53000'\n[http]\nbase_url = 'http://localhost:53001'",
            environment,
        )
        .expect_err("nested fields must not escape Sign validation");
        assert!(error.contains("unknown field `http`"), "{error}");
    }
}
