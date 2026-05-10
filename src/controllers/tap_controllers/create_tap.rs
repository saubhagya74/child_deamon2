use std::process::Command;
use std::str;
use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use sqlsrv::rusqlite::params;

use crate::controllers::tap_controllers::{ManageTap, TapError};
use crate::db_services;
use crate::my_states::{AppState, OperState};

#[derive(Serialize,Deserialize)]
pub struct TapInfo {
    pub id: u64,
    pub bridge_id: Option<u64>,
    pub operstate: OperState,
    pub created_at: i64,
}
pub async fn handle_create_tap(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match ManageTap::create_tap(&state).await {
        Ok(tap_info) => (StatusCode::CREATED, Json(tap_info)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(format!("Tap creation failed: {:?}", e))).into_response(),
    }
}

impl ManageTap{

    pub async fn create_tap(
        state: &AppState
    ) -> Result<TapInfo, TapError> {

        let tap_id = (state.id_bucket.lock().get_id() % 10_000_000_000_000) as u64;
        let tap_name = ManageTap::get_tap_name_by_id(tap_id);
        let created_at = chrono::Utc::now().timestamp();

        let mut tap_was_created = false;
        let mut tap_was_set_up = false;

        let result: Result<TapInfo, TapError> = (|| {
            // 1. Create the TAP
            let create_output = Command::new("sudo")
                .arg("ip")
                .arg("tuntap")
                .arg("add")
                .arg(&tap_name)
                .arg("mode")
                .arg("tap")
                // .arg("user")
                // .arg(std::env::var("USER").unwrap_or_else(|_| "root".to_string()))
                .output()
                .map_err(|e| TapError::SystemError(format!("Failed to execute command: {}", e)))?;

            if !create_output.status.success() {
                return Err(TapError::CommandFailed(format!(
                    "Failed to create TAP {}: {}",
                    tap_name, String::from_utf8_lossy(&create_output.stderr)
                )));
            }
            tap_was_created = true;

            // 2. Set Link UP
            let up_output = Command::new("sudo")
                .arg("ip")
                .arg("link")
                .arg("set")
                .arg(&tap_name)
                .arg("up")
                .output()
                .map_err(|e| TapError::SystemError(format!("Failed to execute command: {}", e)))?;

            if !up_output.status.success() {
                return Err(TapError::CommandFailed(format!(
                    "Failed to bring TAP {} up: {}",
                    tap_name, String::from_utf8_lossy(&up_output.stderr)
                )));
            }
            tap_was_set_up = true;

            let operstate = Self::get_tap_operstate(&tap_name)?;

            let conn = db_services::get_conn(&state.db_pool)
                .map_err(|e| TapError::SystemError(format!("db conn: {}", e)))?;

            conn.execute(
                "INSERT INTO tap (id, bridge_id, operstate, created_at) VALUES (?, ?, ?, ?)",
                params![tap_id, None::<u64>, format!("{:?}", operstate), created_at],
            ).map_err(|e| TapError::SystemError(format!("Failed to insert tap record: {}", e)))?;

            Ok(TapInfo {
                id: tap_id,
                bridge_id: None,
                operstate,
                created_at,
            })
        })();

        match result {
            Ok(tap_info) => Ok(tap_info),
            Err(e) => {
                if tap_was_created {
                    let _ = Command::new("sudo")
                        .arg("ip")
                        .arg("tuntap")
                        .arg("del")
                        .arg(&tap_name)
                        .arg("mode")
                        .arg("tap")
                        .output();
                }
                // Return the original error that triggered the "catch"
                Err(e)
            }
        }
    }
    pub fn get_tap_operstate(tap_name: &str) -> Result<OperState, TapError> {
        let output = Command::new("ip")
            .arg("link")
            .arg("show")
            .arg(tap_name)
            .output()
            .map_err(|e| TapError::SystemError(format!("Cmd failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Look for the "UP" flag in the brackets - this means it's Admin Up
        // Example: <BROADCAST,MULTICAST,UP,LOWER_UP>
        if stdout.contains("<") && stdout.split('>').next().map_or(false, |s| s.contains(",UP")) {
            // It's administratively UP even if state is DOWN/NO-CARRIER
            return Ok(OperState::Up); 
        }

        if stdout.contains("state UP") {
            Ok(OperState::Up)
        } else {
            Ok(OperState::Down)
        }
    }
}