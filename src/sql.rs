use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::{cli::SqlSourceArgs, config::PermissionPolicy};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    Read,
    DmlWrite,
    SchemaInspect,
    SchemaChange,
    Explain,
}

impl OperationClass {
    pub fn is_mutating(self) -> bool {
        matches!(self, Self::DmlWrite | Self::SchemaChange)
    }
}

impl std::fmt::Display for OperationClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Read => "read",
            Self::DmlWrite => "dml_write",
            Self::SchemaInspect => "schema_inspect",
            Self::SchemaChange => "schema_change",
            Self::Explain => "explain",
        };

        write!(f, "{name}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementClassification {
    pub class: OperationClass,
    pub keyword: String,
}

pub async fn load_sql_source(source: &SqlSourceArgs) -> Result<String> {
    match (&source.sql, &source.file) {
        (Some(sql), None) => Ok(sql.clone()),
        (None, Some(path)) => tokio::fs::read_to_string(path).await.map_err(Into::into),
        _ => bail!("provide exactly one of `--sql` or `--file`"),
    }
}

pub fn authorize_statement(
    sql: &str,
    allow_write: bool,
    policy: &PermissionPolicy,
    command: &str,
) -> Result<StatementClassification> {
    let classification = classify_statement(sql)?;
    authorize_operation(policy, classification.class, command)?;

    if classification.class.is_mutating() && !allow_write {
        bail!(
            "`{command}` requires --write for {} statements",
            classification.class
        );
    }

    Ok(classification)
}

pub fn authorize_operation(
    policy: &PermissionPolicy,
    class: OperationClass,
    command: &str,
) -> Result<()> {
    if policy.allows(class) {
        return Ok(());
    }

    bail!(
        "policy `{}` does not allow {} operations for `{}`",
        policy.name(),
        class,
        command
    )
}

pub fn classify_statement(sql: &str) -> Result<StatementClassification> {
    let stripped = strip_comments_and_literals(sql);
    let tokens = tokenize_sql(&stripped);
    let first = tokens
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("SQL statement is empty"))?;

    let class = match first.as_str() {
        "select" | "values" | "table" => OperationClass::Read,
        "show" | "describe" | "desc" | "pragma" => OperationClass::SchemaInspect,
        "explain" => OperationClass::Explain,
        "insert" | "update" | "delete" | "merge" | "replace" | "copy" | "call" | "do" => {
            OperationClass::DmlWrite
        }
        "create" | "alter" | "drop" | "truncate" | "rename" | "grant" | "revoke" | "comment"
        | "attach" | "detach" | "vacuum" | "reindex" | "cluster" | "analyze" | "set" | "use"
        | "begin" | "commit" | "rollback" | "savepoint" | "release" => OperationClass::SchemaChange,
        "with" => classify_cte(&tokens)?,
        _ => {
            bail!(
                "dbx could not classify the SQL statement safely; leading keyword `{first}` is not supported"
            )
        }
    };

    Ok(StatementClassification {
        class,
        keyword: first,
    })
}

fn classify_cte(tokens: &[String]) -> Result<OperationClass> {
    if let Some(keyword) = tokens
        .iter()
        .skip(1)
        .find_map(|token| match token.as_str() {
            "insert" | "update" | "delete" | "merge" | "replace" | "copy" | "call" | "do" => {
                Some(OperationClass::DmlWrite)
            }
            "create" | "alter" | "drop" | "truncate" | "rename" | "grant" | "revoke"
            | "comment" | "attach" | "detach" | "vacuum" | "reindex" | "cluster" | "analyze"
            | "set" | "use" | "begin" | "commit" | "rollback" | "savepoint" | "release" => {
                Some(OperationClass::SchemaChange)
            }
            "select" | "values" | "table" => Some(OperationClass::Read),
            _ => None,
        })
    {
        return Ok(keyword);
    }

    bail!("dbx could not classify the SQL statement safely; unsupported CTE body")
}

fn tokenize_sql(sql: &str) -> Vec<String> {
    sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn strip_comments_and_literals(sql: &str) -> String {
    enum State {
        Normal,
        LineComment,
        BlockComment,
        SingleQuote,
        DoubleQuote,
        BacktickQuote,
    }

    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut state = State::Normal;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => match ch {
                '-' if chars.peek() == Some(&'-') => {
                    chars.next();
                    state = State::LineComment;
                    result.push(' ');
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    state = State::BlockComment;
                    result.push(' ');
                }
                '\'' => {
                    state = State::SingleQuote;
                    result.push(' ');
                }
                '"' => {
                    state = State::DoubleQuote;
                    result.push(' ');
                }
                '`' => {
                    state = State::BacktickQuote;
                    result.push(' ');
                }
                _ => result.push(ch),
            },
            State::LineComment => {
                if ch == '\n' {
                    state = State::Normal;
                    result.push('\n');
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    state = State::Normal;
                    result.push(' ');
                }
            }
            State::SingleQuote => {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        state = State::Normal;
                        result.push(' ');
                    }
                }
            }
            State::DoubleQuote => {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                    } else {
                        state = State::Normal;
                        result.push(' ');
                    }
                }
            }
            State::BacktickQuote => {
                if ch == '`' {
                    state = State::Normal;
                    result.push(' ');
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    use super::{
        OperationClass, authorize_operation, authorize_statement, classify_statement,
        load_sql_source, strip_comments_and_literals,
    };
    use crate::{cli::SqlSourceArgs, config::PermissionPolicy};

    #[test]
    fn classifies_read_statement() {
        let classification = classify_statement("select * from users").unwrap();
        assert_eq!(classification.class, OperationClass::Read);
        assert_eq!(classification.keyword, "select");
    }

    #[test]
    fn classifies_schema_inspection_statement() {
        let classification = classify_statement("show tables").unwrap();
        assert_eq!(classification.class, OperationClass::SchemaInspect);
    }

    #[test]
    fn classifies_explain_statement() {
        let classification = classify_statement("explain select * from users").unwrap();
        assert_eq!(classification.class, OperationClass::Explain);
    }

    #[test]
    fn classifies_dml_write_statement() {
        let classification = classify_statement("delete from users").unwrap();
        assert_eq!(classification.class, OperationClass::DmlWrite);
    }

    #[test]
    fn classifies_schema_change_statement() {
        let classification =
            classify_statement("alter table users add column active bool").unwrap();
        assert_eq!(classification.class, OperationClass::SchemaChange);
    }

    #[test]
    fn detects_mutation_inside_cte() {
        let sql =
            "with changed as (update users set active = false returning id) select * from changed";
        let classification = classify_statement(sql).unwrap();
        assert_eq!(classification.class, OperationClass::DmlWrite);
    }

    #[test]
    fn ignores_keywords_in_comments_and_literals() {
        let sql = r#"
            -- update users
            select 'drop table users' as sample
        "#;

        let classification = classify_statement(sql).unwrap();
        assert_eq!(classification.class, OperationClass::Read);
    }

    #[test]
    fn strips_comments_and_literals() {
        let sql = r#"select "drop", 'insert', name from users /* delete */ -- drop
where id = 1"#;
        let stripped = strip_comments_and_literals(sql);
        assert!(!stripped.contains("insert"));
        assert!(!stripped.contains("delete"));
        assert!(!stripped.contains("drop"));
        assert!(stripped.contains("where id = 1"));
    }

    #[test]
    fn rejects_empty_sql() {
        let err = classify_statement(" -- only comments").unwrap_err();
        assert!(err.to_string().contains("SQL statement is empty"));
    }

    #[test]
    fn rejects_unknown_leading_keyword() {
        let err = classify_statement("listen channel_updates").unwrap_err();
        assert!(
            err.to_string()
                .contains("leading keyword `listen` is not supported")
        );
    }

    #[test]
    fn authorize_statement_requires_write_flag_for_dml() {
        let policy = PermissionPolicy::builtin("all").unwrap();
        let err = authorize_statement("update users set active = false", false, &policy, "query")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("`query` requires --write for dml_write statements")
        );
    }

    #[test]
    fn authorize_statement_requires_write_flag_for_schema_change() {
        let policy = PermissionPolicy::builtin("all").unwrap();
        let err = authorize_statement("drop table users", false, &policy, "exec").unwrap_err();
        assert!(
            err.to_string()
                .contains("`exec` requires --write for schema_change statements")
        );
    }

    #[test]
    fn authorize_statement_obeys_policy() {
        let policy = PermissionPolicy::builtin("prod_safe").unwrap();
        let err = authorize_statement("delete from users", true, &policy, "exec").unwrap_err();
        assert!(
            err.to_string()
                .contains("policy `prod_safe` does not allow dml_write operations for `exec`")
        );
    }

    #[test]
    fn authorize_operation_allows_schema_inspection_for_readonly_policy() {
        let policy = PermissionPolicy::builtin("readonly").unwrap();
        authorize_operation(&policy, OperationClass::SchemaInspect, "tables").unwrap();
    }

    #[test]
    fn load_sql_source_reads_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "select 1").unwrap();
        let source = SqlSourceArgs {
            sql: None,
            file: Some(file.path().to_path_buf()),
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded = runtime.block_on(load_sql_source(&source)).unwrap();
        assert_eq!(loaded, "select 1");
    }

    #[test]
    fn load_sql_source_rejects_ambiguous_input() {
        let source = SqlSourceArgs {
            sql: Some("select 1".to_owned()),
            file: Some("query.sql".into()),
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let err = runtime.block_on(load_sql_source(&source)).unwrap_err();
        assert!(
            err.to_string()
                .contains("provide exactly one of `--sql` or `--file`")
        );
    }
}
