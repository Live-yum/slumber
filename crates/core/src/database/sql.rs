//! Native SQL execution for the CLI; uses the same bundled SQLite connection.
use super::{Database, DatabaseError};
use fallible_iterator::FallibleIterator;
use rusqlite::{Batch, types::ValueRef};
use serde_json::Value;

/// Rows from one SQL statement, preserving integers and duplicate column names.
pub struct SqlResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}
impl Database {
    /// Run explicit user SQL. Never log SQL text or result values.
    /// Use BEGIN/COMMIT for atomic multi-statement scripts.
    pub fn execute_sql(
        &self,
        sql: &str,
    ) -> Result<Vec<SqlResult>, DatabaseError> {
        let connection = self.connection();
        let mut batch = Batch::new(&connection, sql);
        let mut results = Vec::new();
        while let Some(mut statement) = batch.next()? {
            let columns = statement
                .column_names()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if columns.is_empty() {
                statement.execute([])?;
            } else {
                let mut cursor = statement.query([])?;
                let mut rows = Vec::new();
                while let Some(row) = cursor.next()? {
                    let mut values = Vec::new();
                    for index in 0..columns.len() {
                        values.push(match row.get_ref(index)? {
                            ValueRef::Null => Value::Null,
                            ValueRef::Integer(value) => Value::from(value),
                            ValueRef::Real(value) => Value::from(value),
                            ValueRef::Text(value) => Value::from(
                                String::from_utf8_lossy(value).as_ref(),
                            ),
                            ValueRef::Blob(value) => {
                                use std::fmt::Write as _;
                                let mut hex = String::from("X'");
                                for byte in value {
                                    write!(&mut hex, "{byte:02x}")
                                        .expect("writing to String");
                                }
                                hex.push('\'');
                                Value::from(hex)
                            }
                        });
                    }
                    rows.push(values);
                }
                results.push(SqlResult { columns, rows });
            }
        }
        Ok(results)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sql_multi_statement_and_large_integer() {
        let database = Database {
            connection: std::sync::Arc::new(std::sync::Mutex::new(
                rusqlite::Connection::open_in_memory().unwrap(),
            )),
        };
        let results = database.execute_sql("CREATE TABLE native_test (id INTEGER, text TEXT); INSERT INTO native_test VALUES (9007199254740993, '中文; text'); SELECT * FROM native_test; SELECT X'ff00';").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].rows[0][0],
            serde_json::json!(9_007_199_254_740_993_i64)
        );
        assert_eq!(results[0].rows[0][1], "中文; text");
        assert_eq!(results[1].rows[0][0], "X'ff00'");
        assert!(database.execute_sql("not SQL").is_err());
    }
}
