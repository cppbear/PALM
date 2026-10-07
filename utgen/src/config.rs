//! Runtime model configuration. Loading never makes a network request.

use serde::Deserialize;
use std::{env, ffi::OsString, fmt, fs, path::Path};

#[derive(Clone)]
pub struct LlmConfig {
    pub(crate) base: String,
    pub(crate) key: String,
    pub(crate) model: String,
}

// Do not expose credentials through logs or a failed assertion.
impl fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConfig").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    base: Option<String>,
    key: Option<String>,
    model: Option<String>,
}

impl LlmConfig {
    /// Select a file with `--config`, falling back to `PALM_CONFIG`.
    /// Individual `PALM_API_BASE`, `PALM_API_KEY`, and `PALM_MODEL` values
    /// override file fields. No implicit current-directory file is loaded.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        Self::load_with_env(path, |name| env::var_os(name))
    }

    fn load_with_env(
        path: Option<&Path>,
        env_value: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, ConfigError> {
        let env_path = env_value("PALM_CONFIG");
        let selected_path = path.or_else(|| env_path.as_deref().map(Path::new));
        let file: ConfigFile = if let Some(path) = selected_path {
            let contents = fs::read_to_string(path).map_err(|err| {
                ConfigError(format!(
                    "Cannot read model configuration {}: {err}",
                    path.display()
                ))
            })?;
            serde_json::from_str(&contents).map_err(|err| {
                // serde's detailed message can contain values from the file.
                ConfigError(format!(
                    "Invalid model configuration {} at line {}, column {}; expected a JSON object with string fields base, key, model",
                    path.display(), err.line(), err.column()
                ))
            })?
        } else {
            ConfigFile::default()
        };

        let field = |name: &str, value: Option<String>| -> Result<String, ConfigError> {
            let value = match env_value(name) {
                Some(value) => Some(
                    value
                        .into_string()
                        .map_err(|_| ConfigError(format!("{name} must contain valid Unicode")))?,
                ),
                None => value,
            };
            match value {
                Some(value) if !value.trim().is_empty() => Ok(value),
                _ => Err(ConfigError(format!(
                    "Missing or empty {name}; set it or provide --config <path> / PALM_CONFIG with base, key, model"
                ))),
            }
        };

        Ok(Self {
            base: field("PALM_API_BASE", file.base)?,
            key: field("PALM_API_KEY", file.key)?,
            model: field("PALM_MODEL", file.model)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

    struct ConfigFixture(PathBuf);

    impl ConfigFixture {
        fn new(contents: &str) -> Self {
            let path = env::temp_dir().join(format!(
                "palm-config-{}-{}.json",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for ConfigFixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn variables(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn loads_legacy_file_fields_at_runtime() {
        let fixture = ConfigFixture::new(
            r#"{"base":"http://localhost/v1","key":"test-only","model":"file-model"}"#,
        );
        let config = LlmConfig::load_with_env(Some(&fixture.0), |_| None).unwrap();
        assert_eq!(config.model, "file-model");
        fs::write(
            &fixture.0,
            r#"{"base":"http://localhost/v1","key":"test-only","model":"changed-model"}"#,
        )
        .unwrap();
        let changed = LlmConfig::load_with_env(Some(&fixture.0), |_| None).unwrap();
        assert_eq!(changed.model, "changed-model");
    }

    #[test]
    fn environment_fields_override_file_and_explicit_path_wins() {
        let fixture = ConfigFixture::new(
            r#"{"base":"http://localhost/v1","key":"file-key","model":"file-model"}"#,
        );
        let config = LlmConfig::load_with_env(
            Some(&fixture.0),
            variables(&[
                ("PALM_CONFIG", "/does-not-exist/ignored.json"),
                ("PALM_API_KEY", "env-key"),
                ("PALM_MODEL", "env-model"),
            ]),
        )
        .unwrap();
        assert_eq!(config.base, "http://localhost/v1");
        assert_eq!(config.key, "env-key");
        assert_eq!(config.model, "env-model");
    }

    #[test]
    fn accepts_environment_only_and_environment_selected_file() {
        let config = LlmConfig::load_with_env(
            None,
            variables(&[
                ("PALM_API_BASE", "http://localhost/v1"),
                ("PALM_API_KEY", "env-key"),
                ("PALM_MODEL", "env-model"),
            ]),
        )
        .unwrap();
        assert_eq!(config.model, "env-model");
        let fixture = ConfigFixture::new(
            r#"{"base":"http://localhost/v1","key":"test-only","model":"file-model"}"#,
        );
        let config = LlmConfig::load_with_env(
            None,
            variables(&[("PALM_CONFIG", fixture.0.to_str().unwrap())]),
        )
        .unwrap();
        assert_eq!(config.model, "file-model");
    }

    #[test]
    fn reports_missing_and_empty_fields_without_falling_back() {
        assert!(
            LlmConfig::load_with_env(None, |_| None)
                .unwrap_err()
                .to_string()
                .contains("PALM_API_BASE")
        );
        let fixture = ConfigFixture::new(
            r#"{"base":"http://localhost/v1","key":"file-key","model":"file-model"}"#,
        );
        let err = LlmConfig::load_with_env(Some(&fixture.0), variables(&[("PALM_API_KEY", " ")]))
            .unwrap_err();
        assert!(err.to_string().contains("PALM_API_KEY"));
        let err =
            LlmConfig::load_with_env(Some(Path::new("/does-not-exist/palm-config.json")), |_| {
                None
            })
            .unwrap_err();
        assert!(err.to_string().contains("Cannot read"));
    }

    #[test]
    fn configuration_errors_and_debug_do_not_disclose_values() {
        let fixture = ConfigFixture::new(
            r#"{"base":"http://localhost/v1","key":{"never-print-me":1},"model":"mock"}"#,
        );
        let err = LlmConfig::load_with_env(Some(&fixture.0), |_| None).unwrap_err();
        assert!(!err.to_string().contains("never-print-me"));
        let config = LlmConfig {
            base: "private-base".into(),
            key: "never-print-me".into(),
            model: "private-model".into(),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("private"));
        assert!(!debug.contains("never-print-me"));
    }
}
