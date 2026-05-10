
use std::sync::Arc;
use sqlsrv::{SqliteConnectionManager, r2d2};

pub fn init_db_pool(db_path: &str) -> Result<Arc<r2d2::Pool<SqliteConnectionManager>>, DbError> {

    let manager = SqliteConnectionManager::file(db_path);
    let pool = r2d2::Pool::new(manager)
        .map_err(|e| DbError::PoolError(format!("Failed to create pool: {}", e)))?;
    
    run_migrations(&pool)?;
    
    Ok(Arc::new(pool))
}

pub fn get_conn(pool: &Arc<r2d2::Pool<SqliteConnectionManager>>) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, DbError> {
    pool.get()
        .map_err(|e| DbError::ConnectionError(format!("Failed to get connection: {}", e)))
}

fn run_migrations(pool: &r2d2::Pool<SqliteConnectionManager>) -> Result<(), DbError> {
    let conn = pool.get()
        .map_err(|e| DbError::ConnectionError(format!("Failed to get connection for migration: {}", e)))?;
    //put a fallback to delete the db if migraiton fails???
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS vm (
            id          INTEGER PRIMARY KEY, 
            ip          TEXT NOT NULL,
            tap_id      INTEGER,
            bridge_id   INTEGER,
            disk_path   TEXT,
            pid         INTEGER,
            status      TEXT,
            memory_mb   INTEGER,
            created_at  INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE TABLE IF NOT EXISTS bridge (
            id          INTEGER PRIMARY KEY,
            cidr        INTEGER NOT NULL,
            operstate     TEXT,
            created_at  INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE IF NOT EXISTS bridge_ip (
            bridge_id INTEGER NOT NULL,
            ip        INTEGER NOT NULL,
            UNIQUE(bridge_id, ip)
        );

        CREATE TABLE IF NOT EXISTS recycle_bridge_ip (
            bridge_id INTEGER NOT NULL,
            ip        INTEGER NOT NULL,
            UNIQUE(bridge_id, ip)
        );
        CREATE TABLE IF NOT EXISTS tap (
            id          INTEGER PRIMARY KEY,
            bridge_id   INTEGER,
            operstate      TEXT,
            created_at  INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE TABLE IF NOT EXISTS ip_pool (
            ip          INTEGER PRIMARY KEY,
            cidr        INTEGER NOT NULL,
            dev_id      INTEGER,
            slots_left  INTEGER,
            assigned_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS recycle_ip_pool (
            ip          INTEGER PRIMARY KEY,
            cidr        INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS vsock_id (
            id INTEGER NOT NULL
        );
        
        CREATE TABLE IF NOT EXISTS recycle_vsock_id (
            beg_vsock_id INTEGER NOT NULL,
            end_vsock_id INTEGER NOT NULL,
            CHECK(beg_vsock_id <= end_vsock_id)
        );

        CREATE INDEX IF NOT EXISTS idx_recycle_beg ON recycle_vsock_id(beg_vsock_id);
        CREATE INDEX IF NOT EXISTS idx_recycle_end ON recycle_vsock_id(end_vsock_id);

        CREATE TABLE IF NOT EXISTS recycle_ip_pool (
            ip          INTEGER PRIMARY KEY,
            cidr        INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ipset(
            id            INTEGER PRIMARY KEY,
            type          TEXT NOT NULL,
            cidr INTEGER
        );
        create table if not exists ipset_ips(
            ipset_id INTEGER,
            ip INTEGER,
            UNIQUE(ipset_id, ip)
        );
        CREATE TABLE IF NOT EXISTS far_ip_pool (
            ip          TEXT PRIMARY KEY,
            cidr        INTEGER NOT NULL,
            bridge_id   INTEGER,
            assigned_at INTEGER
        );
    ").map_err(|e| DbError::MigrationError(format!("Migration failed: {}", e)))?;
    //far ip starts from really far and is assigned without doing cidr go backward from here 172.31.255.255
    //ip pool is the cidr range
    Ok(())
}
pub trait DbConnection {
    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, DbError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Query error: {0}")]
    QueryError(String),
    
    #[error("Pool error: {0}")]
    PoolError(String),
    
    #[error("Migration error: {0}")]
    MigrationError(String),
}