use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};

use crate::output::OutputFormat;

#[derive(Debug, Parser)]
#[command(
    name = "dbx",
    version,
    about = "Direct multi-database CLI for MySQL, PostgreSQL, and SQLite"
)]
pub struct Cli {
    #[arg(long, global = true, env = "DBX_CONFIG", value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, env = "DBX_PROFILE", value_name = "NAME")]
    pub profile: Option<String>,

    #[arg(long, global = true, value_enum)]
    pub format: Option<OutputFormat>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Query(StatementArgs),
    Exec(StatementArgs),
    Tables(TablesArgs),
    #[command(visible_alias = "desc")]
    Schema(SchemaArgs),
    Explain(SqlSourceArgs),
}

#[derive(Debug, Args)]
pub struct TablesArgs {
    #[arg(long, value_name = "SCHEMA")]
    pub schema: Option<String>,
}

#[derive(Debug, Args)]
pub struct SchemaArgs {
    pub table: String,

    #[arg(long, value_name = "SCHEMA")]
    pub schema: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatementArgs {
    #[command(flatten)]
    pub source: SqlSourceArgs,

    #[arg(long, help = "Opt in to obvious mutating statements")]
    pub write: bool,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("input")
        .required(true)
        .args(["sql", "file"])
))]
pub struct SqlSourceArgs {
    #[arg(long, value_name = "SQL", conflicts_with = "file")]
    pub sql: Option<String>,

    #[arg(long, value_name = "PATH", conflicts_with = "sql")]
    pub file: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands};

    #[test]
    fn parses_query_command_with_write_flag() {
        let cli = Cli::try_parse_from([
            "dbx",
            "--profile",
            "prod",
            "query",
            "--write",
            "--sql",
            "update users set active = false",
        ])
        .unwrap();

        assert_eq!(cli.profile.as_deref(), Some("prod"));
        assert!(matches!(
            cli.command,
            Commands::Query(args) if args.write && args.source.sql.as_deref() == Some("update users set active = false")
        ));
    }

    #[test]
    fn parses_schema_alias() {
        let cli = Cli::try_parse_from(["dbx", "desc", "users"]).unwrap();

        assert!(matches!(
            cli.command,
            Commands::Schema(args) if args.table == "users"
        ));
    }

    #[test]
    fn rejects_missing_sql_source() {
        let err = Cli::try_parse_from(["dbx", "exec"]).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--sql"));
        assert!(text.contains("--file"));
    }
}
