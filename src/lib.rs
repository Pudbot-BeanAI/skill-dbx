pub mod cli;
pub mod config;
pub mod db;
pub mod output;
pub mod sql;

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands};
use config::{Config, PermissionPolicy};
use db::DatabaseClient;
use output::print_output;
use sql::{OperationClass, authorize_operation, authorize_statement, load_sql_source};

#[derive(Debug)]
enum PreparedCommand {
    Query {
        sql: String,
    },
    Exec {
        sql: String,
    },
    Tables {
        schema: Option<String>,
    },
    Schema {
        table: String,
        schema: Option<String>,
    },
    Explain {
        sql: String,
    },
}

pub async fn main_entry() -> ExitCode {
    sqlx::any::install_default_drivers();

    match run_cli(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

pub async fn run_cli(cli: Cli) -> Result<()> {
    let config_path = config::resolve_config_path(cli.config.as_deref())?;
    let config = Config::load(&config_path)
        .with_context(|| format!("failed to load config file `{}`", config_path.display()))?;
    let profile = config.resolve_profile(cli.profile.as_deref())?;
    let format = cli
        .format
        .or(config.output.default_format)
        .unwrap_or_default();
    let command = prepare_command(&cli.command, &profile.policy).await?;
    let client = DatabaseClient::connect(profile.name, profile.config)
        .await
        .with_context(|| format!("failed to connect using profile `{}`", profile.name))?;

    let output = match command {
        PreparedCommand::Query { sql } => client.query(&sql).await?,
        PreparedCommand::Exec { sql } => client.exec(&sql).await?,
        PreparedCommand::Tables { schema } => client.tables(schema.as_deref()).await?,
        PreparedCommand::Schema { table, schema } => {
            client.schema(&table, schema.as_deref()).await?
        }
        PreparedCommand::Explain { sql } => client.explain(&sql).await?,
    };

    print_output(&output, format)?;
    Ok(())
}

async fn prepare_command(command: &Commands, policy: &PermissionPolicy) -> Result<PreparedCommand> {
    match command {
        Commands::Query(args) => {
            let sql = load_sql_source(&args.source).await?;
            authorize_statement(&sql, args.write, policy, "query")?;
            Ok(PreparedCommand::Query { sql })
        }
        Commands::Exec(args) => {
            let sql = load_sql_source(&args.source).await?;
            authorize_statement(&sql, args.write, policy, "exec")?;
            Ok(PreparedCommand::Exec { sql })
        }
        Commands::Tables(args) => {
            authorize_operation(policy, OperationClass::SchemaInspect, "tables")?;
            Ok(PreparedCommand::Tables {
                schema: args.schema.clone(),
            })
        }
        Commands::Schema(args) => {
            authorize_operation(policy, OperationClass::SchemaInspect, "schema")?;
            Ok(PreparedCommand::Schema {
                table: args.table.clone(),
                schema: args.schema.clone(),
            })
        }
        Commands::Explain(args) => {
            let sql = load_sql_source(args).await?;
            authorize_operation(policy, OperationClass::Explain, "explain")?;
            Ok(PreparedCommand::Explain { sql })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use tempfile::tempdir;

    use super::{PreparedCommand, prepare_command};
    use crate::{
        cli::{Commands, SchemaArgs, SqlSourceArgs, StatementArgs, TablesArgs},
        config::PermissionPolicy,
        output::OutputFormat,
        run_cli,
    };

    fn sql_source(sql: &str) -> SqlSourceArgs {
        SqlSourceArgs {
            sql: Some(sql.to_owned()),
            file: None,
        }
    }

    fn install_drivers() {
        static ONCE: OnceLock<()> = OnceLock::new();
        ONCE.get_or_init(sqlx::any::install_default_drivers);
    }

    fn sqlite_url(path: &std::path::Path) -> String {
        format!("sqlite://{}", path.display())
    }

    #[tokio::test]
    async fn prepare_command_allows_schema_commands_for_readonly_policy() {
        let policy = PermissionPolicy::builtin("readonly").unwrap();
        let command = Commands::Tables(TablesArgs {
            schema: Some("public".to_owned()),
        });

        let prepared = prepare_command(&command, &policy).await.unwrap();
        assert!(matches!(
            prepared,
            PreparedCommand::Tables { schema } if schema.as_deref() == Some("public")
        ));
    }

    #[tokio::test]
    async fn prepare_command_blocks_query_when_policy_denies_write() {
        let policy = PermissionPolicy::builtin("prod_safe").unwrap();
        let command = Commands::Query(StatementArgs {
            source: sql_source("update users set active = false"),
            write: true,
        });

        let err = prepare_command(&command, &policy).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("policy `prod_safe` does not allow dml_write operations for `query`")
        );
    }

    #[tokio::test]
    async fn prepare_command_requires_write_for_mutation_even_when_policy_allows_it() {
        let policy = PermissionPolicy::builtin("all").unwrap();
        let command = Commands::Exec(StatementArgs {
            source: sql_source("create table widgets(id integer)"),
            write: false,
        });

        let err = prepare_command(&command, &policy).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("`exec` requires --write for schema_change statements")
        );
    }

    #[tokio::test]
    async fn prepare_command_blocks_explain_when_policy_denies_it() {
        let policy =
            PermissionPolicy::new("migration_only", [crate::sql::OperationClass::SchemaChange]);
        let command = Commands::Explain(sql_source("select * from users"));

        let err = prepare_command(&command, &policy).await.unwrap_err();
        assert!(
            err.to_string().contains(
                "policy `migration_only` does not allow explain operations for `explain`"
            )
        );
    }

    #[tokio::test]
    async fn prepare_command_keeps_schema_arguments() {
        let policy = PermissionPolicy::builtin("readonly").unwrap();
        let command = Commands::Schema(SchemaArgs {
            table: "users".to_owned(),
            schema: Some("public".to_owned()),
        });

        let prepared = prepare_command(&command, &policy).await.unwrap();
        assert!(matches!(
            prepared,
            PreparedCommand::Schema { table, schema }
                if table == "users" && schema.as_deref() == Some("public")
        ));
    }

    #[tokio::test]
    async fn run_cli_executes_sqlite_commands_end_to_end() {
        install_drivers();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        let config_path = dir.path().join("dbx.toml");
        std::fs::File::create(&db_path).unwrap();

        std::fs::write(
            &config_path,
            format!(
                r#"
default_profile = "local"

[profiles.local]
kind = "sqlite"
url = "{}"
policy = "all"

[profiles.prod]
kind = "sqlite"
url = "{}"
policy = "prod_safe"
"#,
                sqlite_url(&db_path),
                sqlite_url(&db_path),
            ),
        )
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Json),
            command: Commands::Exec(StatementArgs {
                source: sql_source("create table users(id integer primary key, name text)"),
                write: true,
            }),
        })
        .await
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Table),
            command: Commands::Exec(StatementArgs {
                source: sql_source("insert into users(name) values ('alice')"),
                write: true,
            }),
        })
        .await
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Json),
            command: Commands::Query(StatementArgs {
                source: sql_source("select id, name from users"),
                write: false,
            }),
        })
        .await
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Table),
            command: Commands::Tables(TablesArgs { schema: None }),
        })
        .await
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Table),
            command: Commands::Schema(SchemaArgs {
                table: "users".to_owned(),
                schema: None,
            }),
        })
        .await
        .unwrap();

        run_cli(crate::cli::Cli {
            config: Some(config_path.clone()),
            profile: None,
            format: Some(OutputFormat::Json),
            command: Commands::Explain(sql_source("select * from users")),
        })
        .await
        .unwrap();

        let err = run_cli(crate::cli::Cli {
            config: Some(config_path),
            profile: Some("prod".to_owned()),
            format: Some(OutputFormat::Json),
            command: Commands::Exec(StatementArgs {
                source: sql_source("delete from users"),
                write: true,
            }),
        })
        .await
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("policy `prod_safe` does not allow dml_write operations for `exec`")
        );
    }
}
