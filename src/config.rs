use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::output::OutputFormat;
use crate::sql::OperationClass;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub policies: BTreeMap<String, PolicyConfig>,

    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub default_format: Option<OutputFormat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileConfig {
    pub kind: DatabaseKind,
    pub url: String,

    #[serde(default)]
    pub policy: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub allow: BTreeSet<OperationClass>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseKind {
    Mysql,
    Postgres,
    Sqlite,
}

#[derive(Debug)]
pub struct ResolvedProfile<'a> {
    pub name: &'a str,
    pub config: &'a ProfileConfig,
    pub policy: PermissionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    name: String,
    allowed: BTreeSet<OperationClass>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("unable to read `{}`", path.display()))?;
        let parsed = toml::from_str::<Self>(&raw)
            .with_context(|| format!("invalid TOML in `{}`", path.display()))?;

        if parsed.profiles.is_empty() {
            bail!("config must define at least one profile");
        }

        Ok(parsed)
    }

    pub fn resolve_profile<'a>(
        &'a self,
        requested: Option<&'a str>,
    ) -> Result<ResolvedProfile<'a>> {
        let name = match requested {
            Some(name) => name,
            None => match self.default_profile.as_deref() {
                Some(name) => name,
                None if self.profiles.len() == 1 => self
                    .profiles
                    .keys()
                    .next()
                    .map(String::as_str)
                    .ok_or_else(|| anyhow!("config must define at least one profile"))?,
                None => {
                    bail!(
                        "multiple profiles found; set `default_profile` in the config or pass `--profile`"
                    )
                }
            },
        };

        let config = self
            .profiles
            .get(name)
            .ok_or_else(|| anyhow!("profile `{name}` not found"))?;
        let policy_name = config.policy.as_deref().unwrap_or("all");
        let policy = self.resolve_policy(policy_name)?;

        Ok(ResolvedProfile {
            name,
            config,
            policy,
        })
    }

    fn resolve_policy(&self, name: &str) -> Result<PermissionPolicy> {
        if let Some(policy) = self.policies.get(name) {
            return Ok(PermissionPolicy::new(name, policy.allow.iter().copied()));
        }

        PermissionPolicy::builtin(name).ok_or_else(|| anyhow!("policy `{name}` not found"))
    }
}

pub fn resolve_config_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    let local = PathBuf::from("dbx.toml");
    if local.exists() {
        return Ok(local);
    }

    let Some(mut config_dir) = dirs::config_dir() else {
        bail!("could not determine a default config path; pass `--config` explicitly");
    };

    config_dir.push("dbx");
    config_dir.push("config.toml");
    Ok(config_dir)
}

impl fmt::Display for DatabaseKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        };

        write!(f, "{value}")
    }
}

impl PermissionPolicy {
    pub fn new<I>(name: impl Into<String>, allowed: I) -> Self
    where
        I: IntoIterator<Item = OperationClass>,
    {
        Self {
            name: name.into(),
            allowed: allowed.into_iter().collect(),
        }
    }

    pub fn builtin(name: &str) -> Option<Self> {
        let policy = match name {
            "all" => Self::new(
                name,
                [
                    OperationClass::Read,
                    OperationClass::DmlWrite,
                    OperationClass::SchemaInspect,
                    OperationClass::SchemaChange,
                    OperationClass::Explain,
                ],
            ),
            "readonly" | "prod_safe" => Self::new(
                name,
                [
                    OperationClass::Read,
                    OperationClass::SchemaInspect,
                    OperationClass::Explain,
                ],
            ),
            "migration_only" => Self::new(name, [OperationClass::SchemaChange]),
            _ => return None,
        };

        Some(policy)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn allows(&self, class: OperationClass) -> bool {
        self.allowed.contains(&class)
    }

    pub fn allowed(&self) -> &BTreeSet<OperationClass> {
        &self.allowed
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    use super::{Config, DatabaseKind, PermissionPolicy, resolve_config_path};
    use crate::sql::OperationClass;
    use tempfile::tempdir;

    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_config(contents: &str) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), contents).unwrap();
        file
    }

    #[test]
    fn loads_config_and_resolves_custom_policy() {
        let file = write_config(
            r#"
default_profile = "prod"

[policies.readonly_custom]
allow = ["read", "schema_inspect"]

[profiles.prod]
kind = "postgres"
url = "postgres://localhost/app"
policy = "readonly_custom"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let profile = config.resolve_profile(None).unwrap();

        assert_eq!(profile.name, "prod");
        assert_eq!(profile.config.kind, DatabaseKind::Postgres);
        assert!(profile.policy.allows(OperationClass::Read));
        assert!(profile.policy.allows(OperationClass::SchemaInspect));
        assert!(!profile.policy.allows(OperationClass::Explain));
    }

    #[test]
    fn resolves_single_profile_without_default() {
        let file = write_config(
            r#"
[profiles.local]
kind = "sqlite"
url = "sqlite://dev.db"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let profile = config.resolve_profile(None).unwrap();
        assert_eq!(profile.name, "local");
        assert_eq!(profile.config.kind, DatabaseKind::Sqlite);
        assert_eq!(profile.policy.name(), "all");
    }

    #[test]
    fn errors_when_multiple_profiles_have_no_default() {
        let file = write_config(
            r#"
[profiles.a]
kind = "sqlite"
url = "sqlite://a.db"

[profiles.b]
kind = "mysql"
url = "mysql://localhost/b"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(None).unwrap_err();
        assert!(err.to_string().contains(
            "multiple profiles found; set `default_profile` in the config or pass `--profile`"
        ));
    }

    #[test]
    fn errors_on_unknown_profile() {
        let file = write_config(
            r#"
[profiles.local]
kind = "sqlite"
url = "sqlite://dev.db"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(Some("missing")).unwrap_err();
        assert!(err.to_string().contains("profile `missing` not found"));
    }

    #[test]
    fn errors_on_unknown_policy() {
        let file = write_config(
            r#"
[profiles.prod]
kind = "postgres"
url = "postgres://localhost/app"
policy = "missing"
"#,
        );

        let config = Config::load(file.path()).unwrap();
        let err = config.resolve_profile(None).unwrap_err();
        assert!(err.to_string().contains("policy `missing` not found"));
    }

    #[test]
    fn builtin_policies_have_expected_permissions() {
        let readonly = PermissionPolicy::builtin("readonly").unwrap();
        let migration = PermissionPolicy::builtin("migration_only").unwrap();

        assert!(readonly.allows(OperationClass::Read));
        assert!(readonly.allows(OperationClass::Explain));
        assert!(!readonly.allows(OperationClass::DmlWrite));
        assert!(migration.allows(OperationClass::SchemaChange));
        assert_eq!(migration.allowed().len(), 1);
    }

    #[test]
    fn load_requires_at_least_one_profile() {
        let file = write_config(
            r#"
[output]
default_format = "json"
"#,
        );

        let err = Config::load(file.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("config must define at least one profile")
        );
    }

    #[test]
    fn resolve_config_path_prefers_explicit_path() {
        let explicit = PathBuf::from("/tmp/dbx-explicit.toml");
        let resolved = resolve_config_path(Some(&explicit)).unwrap();
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_config_path_prefers_local_file() {
        let _guard = cwd_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::write("dbx.toml", "ignored").unwrap();

        let resolved = resolve_config_path(None).unwrap();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(resolved, PathBuf::from("dbx.toml"));
    }
}
