use std::process::Command;
use axum::{Json, extract::{State, Path}, response::IntoResponse};
use hyper::StatusCode;
use sqlsrv::rusqlite::params;

use crate::{
    controllers::bridge_controllers::{BridgeError, ManageBridge},
    db_services::get_conn,
    my_states::AppState,
};

pub async fn handle_delete_bridge(
    State(state): State<AppState>,
    axum::extract::Path(bridge_name): axum::extract::Path<String>,
) -> impl IntoResponse {
    match ManageBridge::delete_bridge(&state, bridge_name) {
        Ok(_)  => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(format!("{:?}", e))).into_response(),
    }
}

impl ManageBridge {
    pub fn delete_bridge(state: &AppState, bridge_name: String) -> Result<(), BridgeError> {
        
        let bridge_id = Self::get_bridge_id_by_name(&bridge_name)
            .ok_or_else(|| BridgeError::NotFound(
                format!("Could not parse bridge ID from name: '{}'", bridge_name)
            ))?;

        let conn = get_conn(&state.db_pool)
            .map_err(|e| BridgeError::SystemError(format!("DB connection failed: {}", e)))?;

        // ── 1. Verify the bridge actually exists in DB before doing anything ─
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM bridge WHERE id = ?1",
            params![bridge_id as i64],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|e| BridgeError::SystemError(format!("DB query failed: {}", e)))?;

        if !exists {
            return Err(BridgeError::NotFound(format!("Bridge {} not found", bridge_id)));
        }

        // ── 2. Delete from kernel (best-effort: log but don't block DB cleanup)
        let kernel_err: Option<BridgeError> = {
            match Command::new("sudo")
                .args(["ip", "link", "del", &bridge_name])
                .output()
            {
                Err(e) => Some(BridgeError::SystemError(format!("ip link del spawn failed: {}", e))),
                Ok(out) if !out.status.success() => {
                    let msg = String::from_utf8_lossy(&out.stderr).to_string();
                    // Interface already gone (e.g. host rebooted) — not fatal
                    if msg.contains("Cannot find device") || msg.contains("does not exist") {
                        None
                    } else {
                        Some(BridgeError::CommandFailed(msg))
                    }
                }
                Ok(_) => None,
            }
        };

        // ── 3. Delete from DB in one transaction ─────────────────────────────
        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| BridgeError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;

        let db_result: Result<(), BridgeError> = (|| {
            // Taps are attached to this bridge — remove them first (FK order)
            conn.execute(
                "DELETE FROM tap WHERE bridge_id = ?1",
                params![bridge_id as i64],
            ).map_err(|e| BridgeError::SystemError(format!("tap delete failed: {}", e)))?;

            // Free the IP range back (full delete; no recycle needed for /24 blocks)
            conn.execute(
                "DELETE FROM ip_pool WHERE dev_id = ?1",
                params![bridge_id as i64],
            ).map_err(|e| BridgeError::SystemError(format!("ip_pool delete failed: {}", e)))?;

            conn.execute(
                "DELETE FROM bridge WHERE id = ?1",
                params![bridge_id as i64],
            ).map_err(|e| BridgeError::SystemError(format!("bridge delete failed: {}", e)))?;

            Ok(())
        })();

        match db_result {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| {
                        let _ = conn.execute_batch("ROLLBACK");
                        BridgeError::SystemError(format!("COMMIT failed: {}", e))
                    })?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }

        // ── 4. Surface kernel error now (DB is already clean) ────────────────
        if let Some(e) = kernel_err {
            return Err(e);
        }

        Ok(())
    }
}