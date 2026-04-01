use anyhow::Result;
use clap::ValueEnum;
use comfy_table::{Cell, Table, presets::UTF8_FULL};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandOutput {
    ResultSet {
        columns: Vec<String>,
        rows: Vec<Vec<JsonValue>>,
    },
    Exec {
        rows_affected: u64,
    },
}

impl CommandOutput {
    pub fn result_set(columns: Vec<String>, rows: Vec<Vec<JsonValue>>) -> Self {
        Self::ResultSet { columns, rows }
    }

    pub fn exec(rows_affected: u64) -> Self {
        Self::Exec { rows_affected }
    }
}

pub fn print_output(output: &CommandOutput, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(output)?);
        }
        OutputFormat::Table => print_table(output),
    }

    Ok(())
}

fn print_table(output: &CommandOutput) {
    match output {
        CommandOutput::ResultSet { columns, rows } => {
            if columns.is_empty() {
                println!("(0 rows)");
                return;
            }

            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(columns.iter().map(|column| Cell::new(column.as_str())));

            for row in rows {
                table.add_row(row.iter().map(render_cell));
            }

            println!("{table}");
            println!("{} row(s)", rows.len());
        }
        CommandOutput::Exec { rows_affected } => {
            println!("rows affected: {rows_affected}");
        }
    }
}

fn render_cell(value: &JsonValue) -> Cell {
    match value {
        JsonValue::Null => Cell::new("NULL"),
        JsonValue::Bool(value) => Cell::new(value.to_string()),
        JsonValue::Number(value) => Cell::new(value.to_string()),
        JsonValue::String(value) => Cell::new(value.as_str()),
        JsonValue::Array(_) | JsonValue::Object(_) => Cell::new(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CommandOutput, OutputFormat, print_output, render_cell};

    #[test]
    fn command_output_result_set_constructor_sets_shape() {
        match CommandOutput::result_set(vec!["id".to_owned()], vec![vec![json!(1)]]) {
            CommandOutput::ResultSet { columns, rows } => {
                assert_eq!(columns, vec!["id"]);
                assert_eq!(rows, vec![vec![json!(1)]]);
            }
            CommandOutput::Exec { .. } => panic!("expected result set"),
        }
    }

    #[test]
    fn command_output_exec_constructor_sets_rows_affected() {
        match CommandOutput::exec(7) {
            CommandOutput::Exec { rows_affected } => assert_eq!(rows_affected, 7),
            CommandOutput::ResultSet { .. } => panic!("expected exec output"),
        }
    }

    #[test]
    fn render_cell_formats_scalar_and_structured_json() {
        assert_eq!(render_cell(&json!(null)).content(), "NULL");
        assert_eq!(render_cell(&json!(true)).content(), "true");
        assert_eq!(render_cell(&json!(42)).content(), "42");
        assert_eq!(render_cell(&json!("hello")).content(), "hello");
        assert_eq!(render_cell(&json!({"k":"v"})).content(), r#"{"k":"v"}"#);
    }

    #[test]
    fn print_output_supports_json_and_table_formats() {
        print_output(
            &CommandOutput::result_set(vec!["id".to_owned()], vec![vec![json!(1)]]),
            OutputFormat::Json,
        )
        .unwrap();
        print_output(
            &CommandOutput::result_set(vec!["id".to_owned()], vec![vec![json!(1)]]),
            OutputFormat::Table,
        )
        .unwrap();
        print_output(&CommandOutput::exec(3), OutputFormat::Table).unwrap();
        print_output(
            &CommandOutput::result_set(vec![], vec![]),
            OutputFormat::Table,
        )
        .unwrap();
    }
}
