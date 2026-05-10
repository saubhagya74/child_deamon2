use std::process::Command;

use axum::{Json, extract::State, response::IntoResponse};
use hyper::StatusCode;
use sqlsrv::rusqlite::params;

use crate::{controllers::tap_controllers::{ManageTap, TapError}, db_services, my_states::AppState};

pub async fn handle_delete_tap(
    State(state): State<AppState>,
    axum::extract::Path(tap_name): axum::extract::Path<String>,
) -> impl IntoResponse {
    match ManageTap::delete_tap(&state, tap_name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(format!("Delete failed: {:?}", e))).into_response(),
    }
}
impl ManageTap{
    pub async fn delete_tap(
        state: &AppState,
        tap_name: String,
    ) -> Result<(), TapError> {
        // println!("DEBUG: Received delete request for tap: {}", tap_name);
        let tap_id=ManageTap::get_tap_id_by_name(&tap_name).unwrap();
        // Step 2: Check if tap exists in database
        let conn = db_services::get_conn(&state.db_pool)
            .map_err(|e| TapError::SystemError(format!("db conn: {}", e)))?;
        
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tap WHERE id = ?)",
            params![tap_id],
            |row| row.get(0),
        ).unwrap_or(false);
        
        if !exists {
            return Err(TapError::TapNotFound(format!("Tap with id {} not found", tap_id)));
        }
        
        // Step 3: Delete the TAP interface
        let delete_output = Command::new("sudo")
            .arg("ip")
            .arg("tuntap")
            .arg("del")
            .arg(&tap_name)
            .arg("mode")
            .arg("tap")
            .output()
            .map_err(|e| TapError::SystemError(format!("Failed to execute command: {}", e)))?;
        
        if !delete_output.status.success() {
            let error_msg = String::from_utf8_lossy(&delete_output.stderr);
            return Err(TapError::CommandFailed(format!(
                "Failed to delete TAP interface {}: {}",
                tap_name, error_msg
            )));
        }
        
        conn.execute(
            "DELETE FROM tap WHERE id = ?",
            params![tap_id],
        ).map_err(|e| TapError::SystemError(format!("Failed to delete tap record: {}", e)))?;
        
        Ok(())
    }
}