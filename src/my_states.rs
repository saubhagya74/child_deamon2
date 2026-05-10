use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdBucket;
use sqlsrv::{SqliteConnectionManager, r2d2};

use crate::db_services::init_db_pool;


pub async fn initialize_state()->AppState
{   
    let db_pool = init_db_pool("/opt/ihk/child.db").unwrap_or_else(|e| {
        eprintln!("Database initialization failed: {}", e);
        std::process::exit(1);
    });

    let my_bucket_id=Arc::new(Mutex::new(
        snowflake::SnowflakeIdBucket::new(1, 1)));
    
    AppState {
        db_pool: db_pool,
        id_bucket: my_bucket_id
    }
}

#[derive(Clone)]
pub struct AppState{
    pub db_pool: Arc<r2d2::Pool<SqliteConnectionManager>>,
    pub id_bucket:Arc<Mutex<SnowflakeIdBucket>>,
}
#[derive(Debug,PartialEq,Eq,Clone,Copy,Serialize,Deserialize)]
pub enum OperState{
    Up,
    Down,
    Unknown,
    NotPresent,
    LowerLayerDown,
    Testing,
    Dormant,
    SuperUnknown
}
