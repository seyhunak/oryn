//! PostgreSQL persistence for ORYN.
//!
//! Queries are written with SQLx's runtime API (no database needed at compile
//! time). Every project-scoped query takes the project id *and* is reached only
//! after the API layer has resolved the caller's access to that project, so the
//! workspace is the tenant boundary.

pub mod ai;
pub mod analytics;
pub mod execution;
pub mod governance;
pub mod identity;
pub mod projects;
pub mod testing;

use sqlx::postgres::PgPoolOptions;
pub use sqlx::PgPool;
use std::time::Duration;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

impl DbError {
    /// Map unique violations to `Conflict`.
    pub fn from_sqlx(e: sqlx::Error, what: &str) -> Self {
        if let sqlx::Error::Database(db) = &e {
            if db.code().as_deref() == Some("23505") {
                return DbError::Conflict(format!("{what} already exists"));
            }
        }
        DbError::Sqlx(e)
    }
}

pub type DbResult<T> = Result<T, DbError>;

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

/// Pagination input (1-based page).
#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub limit: i64,
    pub offset: i64,
}

impl Page {
    pub fn new(page: Option<i64>, per_page: Option<i64>) -> Self {
        let per = per_page.unwrap_or(25).clamp(1, 200);
        let p = page.unwrap_or(1).max(1);
        Self {
            limit: per,
            offset: (p - 1) * per,
        }
    }
}

pub fn new_id() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

pub(crate) fn opt<T>(r: Result<T, sqlx::Error>) -> DbResult<T> {
    match r {
        Ok(v) => Ok(v),
        Err(sqlx::Error::RowNotFound) => Err(DbError::NotFound),
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

/// Slugify a name: lowercase ASCII alphanumerics separated by single dashes.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let s = out.trim_end_matches('-').to_string();
    if s.is_empty() {
        "item".into()
    } else {
        s.chars().take(60).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs() {
        assert_eq!(super::slugify("Northwind Supply!"), "northwind-supply");
        assert_eq!(super::slugify("  --  "), "item");
    }
}
