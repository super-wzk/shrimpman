use std::{error::Error, path::PathBuf};

use config::{Config, ConfigError, Environment, File, FileFormat};
use tracing_subscriber::EnvFilter;

/// 读取 PROJECT_CONFIG 指定的 TOML，再应用 SHRIMPMAN_ 环境变量覆盖。
pub fn load_config(service: &str) -> Result<Config, ConfigError> {
    let path =
        PathBuf::from(std::env::var_os("PROJECT_CONFIG").unwrap_or_else(|| "config.toml".into()));
    Config::builder()
        .add_source(File::from(path).format(FileFormat::Toml))
        .add_source(environment(service))
        .build()
}

fn environment(service: &str) -> Environment {
    Environment::with_prefix("SHRIMPMAN")
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
        .list_separator(",")
        // 只把端点列表按逗号拆分，日志过滤器等普通字符串必须保持原样。
        .with_list_parse_key(&format!("{service}.lease_kv.endpoints"))
}

/// 初始化结构化日志；显式的 RUST_LOG 优先于服务配置。
pub fn init_tracing(config_filter: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = match std::env::var(EnvFilter::DEFAULT_ENV) {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => config_filter.to_owned(),
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter)?)
        .try_init()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_toml_without_splitting_comma_separated_log_filters() {
        for service in ["sign", "entrance", "world"] {
            let prefix = service.to_ascii_uppercase();
            let source = [
                (format!("SHRIMPMAN_{prefix}__ENABLED"), "true".to_owned()),
                (
                    format!("SHRIMPMAN_{prefix}__LEASE_KV__ENDPOINTS"),
                    "http://first:2379,http://second:2379".to_owned(),
                ),
                (
                    format!("SHRIMPMAN_{prefix}__LOGGING__FILTER"),
                    "warn,shrimpman_runtime=info".to_owned(),
                ),
            ]
            .into_iter()
            .collect();
            let file = format!("[{service}]\nenabled = false");
            let config = Config::builder()
                .add_source(File::from_str(&file, FileFormat::Toml))
                .add_source(environment(service).source(Some(source)))
                .build()
                .unwrap();

            assert!(config.get_bool(&format!("{service}.enabled")).unwrap());
            assert_eq!(
                config
                    .get::<Vec<String>>(&format!("{service}.lease_kv.endpoints"))
                    .unwrap(),
                ["http://first:2379", "http://second:2379"]
            );
            assert_eq!(
                config
                    .get_string(&format!("{service}.logging.filter"))
                    .unwrap(),
                "warn,shrimpman_runtime=info"
            );
        }
    }
}
