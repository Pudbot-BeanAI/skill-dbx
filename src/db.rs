use anyhow::{Result, bail};
use serde_json::{Number, Value as JsonValue};
use sqlx::{AnyPool, Column, Row, TypeInfo, any::AnyRow};

use crate::{
    config::{DatabaseKind, ProfileConfig},
    output::CommandOutput,
};

pub struct DatabaseClient {
    kind: DatabaseKind,
    pool: AnyPool,
}

impl DatabaseClient {
    pub async fn connect(_profile_name: &str, profile: &ProfileConfig) -> Result<Self> {
        let pool = AnyPool::connect(&profile.url).await?;

        Ok(Self {
            kind: profile.kind,
            pool,
        })
    }

    pub async fn query(&self, sql: &str) -> Result<CommandOutput> {
        self.fetch_result_set(sql).await
    }

    pub async fn exec(&self, sql: &str) -> Result<CommandOutput> {
        let result = sqlx::query(sql).execute(&self.pool).await?;
        Ok(CommandOutput::exec(result.rows_affected()))
    }

    pub async fn tables(&self, schema: Option<&str>) -> Result<CommandOutput> {
        let sql = build_tables_sql(self.kind, schema)?;
        self.fetch_result_set(&sql).await
    }

    pub async fn schema(&self, table: &str, schema: Option<&str>) -> Result<CommandOutput> {
        let sql = build_schema_sql(self.kind, table, schema)?;
        self.fetch_result_set(&sql).await
    }

    pub async fn explain(&self, sql: &str) -> Result<CommandOutput> {
        let explain_sql = build_explain_sql(self.kind, sql);

        self.fetch_result_set(&explain_sql).await
    }

    async fn fetch_result_set(&self, sql: &str) -> Result<CommandOutput> {
        let rows = sqlx::query(sql).fetch_all(&self.pool).await?;

        let columns = rows
            .first()
            .map(|row| {
                row.columns()
                    .iter()
                    .map(|column| column.name().to_owned())
                    .collect()
            })
            .unwrap_or_default();

        let values = rows
            .iter()
            .map(|row| {
                (0..row.len())
                    .map(|index| cell_to_json(row, index))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(CommandOutput::result_set(columns, values))
    }
}

fn sqlite_schema_reference(schema: Option<&str>) -> Result<String> {
    let schema = schema.unwrap_or("main");
    if schema
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(schema.to_owned())
    } else {
        bail!("sqlite schema names may only contain ASCII letters, digits, or underscores");
    }
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn build_tables_sql(kind: DatabaseKind, schema: Option<&str>) -> Result<String> {
    Ok(match kind {
        DatabaseKind::Mysql => {
            let schema_filter = schema
                .map(|value| format!("'{}'", escape_sql_literal(value)))
                .unwrap_or_else(|| "DATABASE()".to_owned());

            format!(
                "SELECT TABLE_NAME AS table_name, TABLE_TYPE AS table_type \
                 FROM information_schema.tables \
                 WHERE table_schema = {schema_filter} \
                 ORDER BY TABLE_NAME"
            )
        }
        DatabaseKind::Postgres => {
            let schema_filter = schema
                .map(|value| format!("'{}'", escape_sql_literal(value)))
                .unwrap_or_else(|| "current_schema()".to_owned());

            format!(
                "SELECT table_schema, table_name, table_type \
                 FROM information_schema.tables \
                 WHERE table_schema = {schema_filter} \
                 AND table_type IN ('BASE TABLE', 'VIEW') \
                 ORDER BY table_name"
            )
        }
        DatabaseKind::Sqlite => {
            let schema_ref = sqlite_schema_reference(schema)?;
            format!(
                "SELECT name AS table_name, type AS table_type \
                 FROM {schema_ref}.sqlite_master \
                 WHERE type IN ('table', 'view') \
                 AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name"
            )
        }
    })
}

fn build_schema_sql(kind: DatabaseKind, table: &str, schema: Option<&str>) -> Result<String> {
    let table = escape_sql_literal(table);

    Ok(match kind {
        DatabaseKind::Mysql => {
            let schema_filter = schema
                .map(|value| format!("'{}'", escape_sql_literal(value)))
                .unwrap_or_else(|| "DATABASE()".to_owned());

            format!(
                "SELECT COLUMN_NAME AS column_name, COLUMN_TYPE AS data_type, \
                 IS_NULLABLE AS is_nullable, COLUMN_DEFAULT AS column_default, \
                 COLUMN_KEY AS column_key, EXTRA AS extra \
                 FROM information_schema.columns \
                 WHERE table_schema = {schema_filter} \
                 AND table_name = '{table}' \
                 ORDER BY ORDINAL_POSITION"
            )
        }
        DatabaseKind::Postgres => {
            let schema_filter = schema
                .map(|value| format!("'{}'", escape_sql_literal(value)))
                .unwrap_or_else(|| "current_schema()".to_owned());

            format!(
                "SELECT column_name, data_type, is_nullable, column_default, ordinal_position \
                 FROM information_schema.columns \
                 WHERE table_schema = {schema_filter} \
                 AND table_name = '{table}' \
                 ORDER BY ordinal_position"
            )
        }
        DatabaseKind::Sqlite => {
            let schema_ref = sqlite_schema_reference(schema)?;
            format!("PRAGMA {schema_ref}.table_info('{table}')")
        }
    })
}

fn build_explain_sql(kind: DatabaseKind, sql: &str) -> String {
    match kind {
        DatabaseKind::Mysql | DatabaseKind::Postgres => format!("EXPLAIN {sql}"),
        DatabaseKind::Sqlite => format!("EXPLAIN QUERY PLAN {sql}"),
    }
}

fn cell_to_json(row: &AnyRow, index: usize) -> JsonValue {
    if let Ok(value) = row.try_get::<Option<bool>, _>(index) {
        return value.map(JsonValue::Bool).unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<i16>, _>(index) {
        return value
            .map(Number::from)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<i32>, _>(index) {
        return value
            .map(Number::from)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<i64>, _>(index) {
        return value
            .map(Number::from)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<f32>, _>(index) {
        return value
            .and_then(|value| Number::from_f64(value as f64))
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<f64>, _>(index) {
        return value
            .and_then(Number::from_f64)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<String>, _>(index) {
        return value.map(JsonValue::String).unwrap_or(JsonValue::Null);
    }

    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(index) {
        return value
            .map(|bytes| JsonValue::String(format!("0x{}", hex_string(&bytes))))
            .unwrap_or(JsonValue::Null);
    }

    row.columns()
        .get(index)
        .map(|column| JsonValue::String(format!("<unhandled:{}>", column.type_info().name())))
        .unwrap_or(JsonValue::Null)
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use tempfile::tempdir;

    use super::{
        DatabaseClient, build_explain_sql, build_schema_sql, build_tables_sql, escape_sql_literal,
        hex_string, sqlite_schema_reference,
    };
    use crate::{
        config::{DatabaseKind, ProfileConfig},
        output::CommandOutput,
    };

    fn install_drivers() {
        static ONCE: OnceLock<()> = OnceLock::new();
        ONCE.get_or_init(sqlx::any::install_default_drivers);
    }

    fn sqlite_profile(path: &std::path::Path) -> ProfileConfig {
        ProfileConfig {
            kind: DatabaseKind::Sqlite,
            url: format!("sqlite://{}", path.display()),
            policy: None,
        }
    }

    #[test]
    fn mysql_tables_sql_uses_information_schema() {
        let sql = build_tables_sql(DatabaseKind::Mysql, Some("analytics")).unwrap();
        assert!(sql.contains("information_schema.tables"));
        assert!(sql.contains("table_schema = 'analytics'"));
    }

    #[test]
    fn postgres_tables_sql_defaults_to_current_schema() {
        let sql = build_tables_sql(DatabaseKind::Postgres, None).unwrap();
        assert!(sql.contains("current_schema()"));
        assert!(sql.contains("table_type IN ('BASE TABLE', 'VIEW')"));
    }

    #[test]
    fn sqlite_tables_sql_uses_requested_schema_reference() {
        let sql = build_tables_sql(DatabaseKind::Sqlite, Some("temp")).unwrap();
        assert!(sql.contains("FROM temp.sqlite_master"));
        assert!(sql.contains("name NOT LIKE 'sqlite_%'"));
    }

    #[test]
    fn mysql_schema_sql_escapes_literals() {
        let sql = build_schema_sql(DatabaseKind::Mysql, "user's", Some("prod")).unwrap();
        assert!(sql.contains("table_schema = 'prod'"));
        assert!(sql.contains("table_name = 'user''s'"));
    }

    #[test]
    fn postgres_schema_sql_defaults_schema() {
        let sql = build_schema_sql(DatabaseKind::Postgres, "users", None).unwrap();
        assert!(sql.contains("current_schema()"));
        assert!(sql.contains("ORDER BY ordinal_position"));
    }

    #[test]
    fn sqlite_schema_sql_uses_pragma() {
        let sql = build_schema_sql(DatabaseKind::Sqlite, "users", Some("main")).unwrap();
        assert_eq!(sql, "PRAGMA main.table_info('users')");
    }

    #[test]
    fn sqlite_schema_reference_validates_names() {
        assert_eq!(sqlite_schema_reference(Some("main_2")).unwrap(), "main_2");
        let err = sqlite_schema_reference(Some("bad-name")).unwrap_err();
        assert!(err.to_string().contains(
            "sqlite schema names may only contain ASCII letters, digits, or underscores"
        ));
    }

    #[test]
    fn escape_sql_literal_doubles_quotes() {
        assert_eq!(escape_sql_literal("we're"), "we''re");
    }

    #[test]
    fn build_explain_sql_varies_by_driver() {
        assert_eq!(
            build_explain_sql(DatabaseKind::Postgres, "select 1"),
            "EXPLAIN select 1"
        );
        assert_eq!(
            build_explain_sql(DatabaseKind::Sqlite, "select 1"),
            "EXPLAIN QUERY PLAN select 1"
        );
    }

    #[test]
    fn hex_string_encodes_bytes() {
        assert_eq!(hex_string(&[0x0a, 0xff, 0x10]), "0aff10");
    }

    #[tokio::test]
    async fn sqlite_client_runs_queries_and_introspection() {
        install_drivers();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        std::fs::File::create(&db_path).unwrap();
        let profile = sqlite_profile(&db_path);
        let client = DatabaseClient::connect("sqlite", &profile).await.unwrap();

        let created = client
            .exec("create table users(id integer primary key, name text)")
            .await
            .unwrap();
        assert!(matches!(
            created,
            CommandOutput::Exec { rows_affected } if rows_affected == 0
        ));

        let inserted = client
            .exec("insert into users(name) values ('alice')")
            .await
            .unwrap();
        assert!(matches!(
            inserted,
            CommandOutput::Exec { rows_affected } if rows_affected == 1
        ));

        let rows = client
            .query("select id, name, 1.5 as ratio, x'0AFF' as blob, null as missing from users")
            .await
            .unwrap();
        match rows {
            CommandOutput::ResultSet { columns, rows } => {
                assert_eq!(columns, vec!["id", "name", "ratio", "blob", "missing"]);
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], serde_json::json!(1));
                assert_eq!(rows[0][1], serde_json::json!("alice"));
                assert_eq!(rows[0][2], serde_json::json!(1.5));
                assert_eq!(rows[0][3], serde_json::json!("0x0aff"));
                assert_eq!(rows[0][4], serde_json::Value::Null);
            }
            CommandOutput::Exec { .. } => panic!("expected result set"),
        }

        let tables = client.tables(None).await.unwrap();
        assert!(matches!(tables, CommandOutput::ResultSet { .. }));

        let schema = client.schema("users", None).await.unwrap();
        assert!(matches!(schema, CommandOutput::ResultSet { .. }));

        let explain = client.explain("select * from users").await.unwrap();
        assert!(matches!(explain, CommandOutput::ResultSet { .. }));
    }
}
