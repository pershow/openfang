use openparlant_types::error::{SiliCrewError, SiliCrewResult};
use rusqlite::Connection;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum SharedDb {
    Sqlite(Arc<Mutex<Connection>>),
    Postgres(Arc<PgPool>),
}

impl From<Arc<Mutex<Connection>>> for SharedDb {
    fn from(value: Arc<Mutex<Connection>>) -> Self {
        Self::Sqlite(value)
    }
}

impl From<Arc<PgPool>> for SharedDb {
    fn from(value: Arc<PgPool>) -> Self {
        Self::Postgres(value)
    }
}

impl SharedDb {
    pub fn open_sqlite(db_path: &Path) -> SiliCrewResult<Self> {
        let conn = Connection::open(db_path).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        Ok(Self::Sqlite(Arc::new(Mutex::new(conn))))
    }

    pub fn open_sqlite_in_memory() -> SiliCrewResult<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        Ok(Self::Sqlite(Arc::new(Mutex::new(conn))))
    }

    pub async fn open_postgres(database_url: &str) -> SiliCrewResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        Ok(Self::Postgres(Arc::new(pool)))
    }

    pub fn sqlite(&self) -> Option<Arc<Mutex<Connection>>> {
        match self {
            Self::Sqlite(conn) => Some(Arc::clone(conn)),
            Self::Postgres(_) => None,
        }
    }

    pub fn postgres(&self) -> Option<Arc<PgPool>> {
        match self {
            Self::Sqlite(_) => None,
            Self::Postgres(pool) => Some(Arc::clone(pool)),
        }
    }

    pub fn is_postgres(&self) -> bool {
        matches!(self, Self::Postgres(_))
    }
}

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("temporary runtime")
            .block_on(future),
    }
}
