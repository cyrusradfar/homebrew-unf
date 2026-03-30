//! Types for database queries and pagination.

use crate::types::SnapshotId;
use chrono::{DateTime, Utc};

/// Cursor for keyset pagination through snapshot history.
#[derive(Debug, Clone)]
pub struct HistoryCursor {
    pub timestamp: DateTime<Utc>,
    pub id: SnapshotId,
}

/// Scope for history queries.
#[derive(Debug)]
pub enum HistoryScope<'a> {
    /// Exact file match.
    File(&'a str),
    /// Directory prefix (recursive). The string must end with '/'.
    Directory(&'a str),
    /// All files.
    All,
}

/// Utility for building dynamic SQL queries with conditions and parameters.
///
/// Handles the common pattern of constructing WHERE clauses dynamically:
/// - Track base query, conditions, and parameters separately
/// - Join conditions with AND
/// - Execute with typed parameter binding
///
/// # Example
/// ```ignore
/// let mut qb = QueryBuilder::new("SELECT * FROM snapshots");
/// if let Some(path) = file_path {
///     qb.add_condition("file_path = ?", path);
/// }
/// if let Some(since) = since_time {
///     qb.add_condition("timestamp >= ?", since);
/// }
/// qb.order_by("timestamp DESC");
/// qb.limit(100);
/// let (sql, params) = qb.build();
/// ```
pub struct QueryBuilder {
    base: String,
    conditions: Vec<String>,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
    order_clause: Option<String>,
    limit_clause: Option<i64>,
}

impl QueryBuilder {
    /// Creates a new query builder with the base SELECT clause.
    pub fn new(base: &str) -> Self {
        Self {
            base: base.to_string(),
            conditions: Vec::new(),
            params: Vec::new(),
            order_clause: None,
            limit_clause: None,
        }
    }

    /// Adds a WHERE condition with a single parameter.
    ///
    /// The SQL fragment should use `?` for parameter binding.
    /// Parameters are added in order and bound by position.
    pub fn add_condition(&mut self, sql: &str, param: impl rusqlite::types::ToSql + 'static) {
        self.conditions.push(sql.to_string());
        self.params.push(Box::new(param));
    }

    /// Adds a WHERE condition with multiple parameters.
    ///
    /// Useful for complex conditions like `(timestamp < ? OR (timestamp = ? AND id < ?))`.
    pub fn add_condition_with_params(
        &mut self,
        sql: &str,
        params: Vec<Box<dyn rusqlite::types::ToSql>>,
    ) {
        self.conditions.push(sql.to_string());
        self.params.extend(params);
    }

    /// Adds a WHERE condition without a parameter (e.g., `IS NULL`).
    #[cfg(test)]
    pub fn add_condition_no_param(&mut self, sql: &str) {
        self.conditions.push(sql.to_string());
    }

    /// Adds an ORDER BY clause. Overwrites any previous ordering.
    pub fn order_by(&mut self, clause: &str) {
        self.order_clause = Some(clause.to_string());
    }

    /// Adds a LIMIT clause.
    pub fn limit(&mut self, n: i64) {
        self.limit_clause = Some(n);
    }

    /// Builds the final SQL string and returns references to parameters.
    ///
    /// Returns (sql_string, vec of parameter references for binding).
    pub fn build(&self) -> (String, Vec<&dyn rusqlite::types::ToSql>) {
        let mut sql = self.base.clone();

        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
        }

        if let Some(ref order) = self.order_clause {
            sql.push_str(" ORDER BY ");
            sql.push_str(order);
        }

        if let Some(limit) = self.limit_clause {
            sql.push_str(" LIMIT ");
            sql.push_str(&limit.to_string());
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            self.params.iter().map(|p| p.as_ref()).collect();

        (sql, param_refs)
    }
}
