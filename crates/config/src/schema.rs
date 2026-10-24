//! schema — the typed configuration model + validation (report §3.8).
//! Built from the TOML document; the single source of truth for desired state.
extern crate alloc;
use crate::toml::{self, Document};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// How the default entry is chosen when the timeout elapses (report §2.7, §3.3).
/// Deterministic and explainable — never an opaque model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyKind {
    /// Always boot the configured default.
    Default,
    /// Prefer the last confirmed-good entry, else the configured default.
    LastGoodThenDefault,
}

/// What to do when a staged boot fails to confirm (report §3.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollbackKind {
    LastGood,
    Fallback,
    None,
}

/// One declared boot entry. `id` matches a Boot Graph `EntryId` slug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySpec {
    pub id: String,
    pub method: String,         // "efi-chainload" | "nixos-generation" | ...
    pub loader: Option<String>, // EFI path for chainload methods
    pub source: Option<String>, // e.g. "bootspec" for generation discovery
}

/// The whole declarative configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub default: String,
    pub timeout_secs: u16,
    pub policy: PolicyKind,
    pub confirm: bool,
    pub max_tries: u8,
    pub rollback: RollbackKind,
    pub entries: Vec<EntrySpec>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Parse {
        line: usize,
        msg: &'static str,
    },
    Missing(&'static str),
    Invalid(&'static str),
    /// Two entries share an id — the SSOT would be ambiguous.
    DuplicateEntry(String),
    /// `default` names an entry id that is not declared.
    UnknownDefault(String),
}

impl Config {
    /// Parse and validate the config from raw ESP bytes.
    pub fn parse(input: &str) -> Result<Config, ConfigError> {
        let doc = toml::parse(input).map_err(|e| ConfigError::Parse {
            line: e.line,
            msg: e.msg,
        })?;
        Self::from_document(&doc)
    }

    fn from_document(doc: &Document) -> Result<Config, ConfigError> {
        let default = doc
            .root_get("default")
            .and_then(|v| v.as_str())
            .ok_or(ConfigError::Missing("default"))?
            .to_string();

        let timeout_secs = match doc.root_get("timeout") {
            Some(v) => u16::try_from(v.as_int().ok_or(ConfigError::Invalid("timeout"))?)
                .map_err(|_| ConfigError::Invalid("timeout"))?,
            None => 5,
        };

        let policy = match doc.root_get("policy").and_then(|v| v.as_str()) {
            None | Some("last-good-then-default") => PolicyKind::LastGoodThenDefault,
            Some("default") => PolicyKind::Default,
            Some(_) => return Err(ConfigError::Invalid("policy")),
        };

        let tx = doc.tables_named("transaction").next();
        let confirm = tx
            .and_then(|t| t.get("confirm"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let max_tries = match tx.and_then(|t| t.get("max_tries")) {
            Some(v) => u8::try_from(v.as_int().ok_or(ConfigError::Invalid("max_tries"))?)
                .map_err(|_| ConfigError::Invalid("max_tries"))?,
            None => 2,
        };
        if max_tries == 0 {
            return Err(ConfigError::Invalid("max_tries"));
        }

        let rollback = match doc
            .tables_named("recovery")
            .next()
            .and_then(|t| t.get("rollback"))
            .and_then(|v| v.as_str())
        {
            None | Some("last-good") => RollbackKind::LastGood,
            Some("fallback") => RollbackKind::Fallback,
            Some("none") => RollbackKind::None,
            Some(_) => return Err(ConfigError::Invalid("rollback")),
        };

        let mut entries = Vec::new();
        for t in doc.tables_named("entry") {
            let id = t
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or(ConfigError::Missing("entry.id"))?
                .to_string();
            if entries.iter().any(|e: &EntrySpec| e.id == id) {
                return Err(ConfigError::DuplicateEntry(id));
            }
            entries.push(EntrySpec {
                id,
                method: t
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("efi-chainload")
                    .to_string(),
                loader: t
                    .get("loader")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                source: t
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            });
        }

        // `default` must resolve to a declared entry OR a well-known dynamic id
        // (e.g. "nixos.current" is resolved at discovery time, not declared here).
        let is_dynamic = default.contains('.');
        if !is_dynamic && !entries.iter().any(|e| e.id == default) {
            return Err(ConfigError::UnknownDefault(default));
        }

        Ok(Config {
            default,
            timeout_secs,
            policy,
            confirm,
            max_tries,
            rollback,
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        default = "windows"
        timeout = 5
        policy  = "last-good-then-default"

        [transaction]
        confirm   = true
        max_tries = 2

        [[entry]]
        id     = "windows"
        method = "efi-chainload"
        loader = "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"

        [recovery]
        rollback = "last-good"
    "#;

    #[test]
    fn parses_and_validates_full_config() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.default, "windows");
        assert_eq!(c.timeout_secs, 5);
        assert_eq!(c.policy, PolicyKind::LastGoodThenDefault);
        assert!(c.confirm);
        assert_eq!(c.max_tries, 2);
        assert_eq!(c.rollback, RollbackKind::LastGood);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(
            c.entries[0].loader.as_deref(),
            Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi")
        );
    }

    #[test]
    fn dynamic_default_is_allowed_without_declared_entry() {
        let c = Config::parse("default = \"nixos.current\"\n").unwrap();
        assert_eq!(c.default, "nixos.current");
    }

    #[test]
    fn rejects_missing_default_bad_values_and_duplicates() {
        assert_eq!(
            Config::parse("timeout = 5\n").unwrap_err(),
            ConfigError::Missing("default")
        );
        assert_eq!(
            Config::parse("default=\"x\"\ntimeout = -1\n").unwrap_err(),
            ConfigError::Invalid("timeout")
        );
        let dup = "default=\"a\"\n[[entry]]\nid=\"a\"\n[[entry]]\nid=\"a\"\n";
        assert_eq!(
            Config::parse(dup).unwrap_err(),
            ConfigError::DuplicateEntry("a".into())
        );
        // default referring to an undeclared, non-dynamic id
        assert_eq!(
            Config::parse("default=\"ghost\"\n").unwrap_err(),
            ConfigError::UnknownDefault("ghost".into())
        );
    }

    #[test]
    fn max_tries_zero_is_rejected() {
        let c = "default=\"a\"\n[[entry]]\nid=\"a\"\n[transaction]\nmax_tries = 0\n";
        assert_eq!(
            Config::parse(c).unwrap_err(),
            ConfigError::Invalid("max_tries")
        );
    }
}
