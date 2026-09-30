//! In-process SQL console. No sqlite3 executable or loadable extensions.
use anyhow::{Context, bail};
use slumber_core::database::Database;
use std::io::{self, BufRead, IsTerminal, Read, Write};

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let database = Database::load()?;
    let mut output = io::stdout().lock();
    if !args.is_empty() {
        for sql in args {
            execute(&database, sql, &mut output)?;
        }
        return Ok(());
    }
    let mut input = io::stdin().lock();
    if !io::stdin().is_terminal() {
        let mut sql = String::new();
        input.read_to_string(&mut sql)?;
        execute(&database, &sql, &mut output)?;
        return Ok(());
    }
    writeln!(
        output,
        "Slumber built-in SQLite. SQL ends with ';'. .tables .schema .help .quit"
    )?;
    let mut sql = String::new();
    loop {
        write!(
            output,
            "{}",
            if sql.is_empty() {
                "slumber> "
            } else {
                "    ...> "
            }
        )?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        if sql.is_empty() && matches!(line.trim(), ".quit" | ".exit") {
            break;
        }
        sql.push_str(&line);
        if sql.trim().starts_with('.') || sql.trim_end().ends_with(';') {
            if let Err(error) = execute(&database, &sql, &mut output) {
                writeln!(output, "Error: {error}")?;
            }
            sql.clear();
        }
    }
    if !sql.trim().is_empty() {
        execute(&database, &sql, &mut output)?;
    }
    Ok(())
}
fn execute(
    database: &Database,
    sql: &str,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    let query = match sql.trim() {
        "" | ".quit" | ".exit" => return Ok(()),
        ".help" => {
            writeln!(
                output,
                "Enter SQL (multiple statements allowed). Results are JSON arrays of rows.\n.tables: list tables; .schema: schema definitions; .quit: exit.\nNo external program or extension loading. Use BEGIN/COMMIT for atomic scripts."
            )?;
            return Ok(());
        }
        ".tables" => {
            "SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"
        }
        ".schema" => {
            "SELECT sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name"
        }
        text if text.starts_with('.') => {
            bail!("Unsupported dot-command. Use .help or SQL PRAGMA statements")
        }
        text => text,
    };
    for result in database
        .execute_sql(query)
        .context("SQL execution failed")?
    {
        serde_json::to_writer(
            &mut *output,
            &serde_json::json!({"columns": result.columns, "rows": result.rows}),
        )?;
        writeln!(output)?;
    }
    Ok(())
}
