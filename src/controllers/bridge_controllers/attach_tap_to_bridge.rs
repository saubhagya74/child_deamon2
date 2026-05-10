use std::process::Command;
use serde::{Serialize, Deserialize};
use sqlsrv::rusqlite::params;

use crate::{
    controllers::{
        bridge_controllers::{BridgeError, ManageBridge},
        tap_controllers::ManageTap,
    },
    db_services::get_conn,
    my_states::{AppState, OperState},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct TapBridgeInfo {
    pub tap_id:      u64,
    pub tap_name:    String,
    pub bridge_id:   u64,
    pub bridge_name: String,
    pub operstate:   OperState,
}

impl ManageBridge {
    pub fn attach_tap_to_bridge(
        state:       &AppState,
        tap_name:    &str,
        bridge_name: &str,
    ) -> Result<TapBridgeInfo, BridgeError> {

        // ── 1. Parse IDs from names ───────────────────────────────────────────
        let bridge_id = Self::get_bridge_id_by_name(bridge_name)
            .ok_or_else(|| BridgeError::NotFound(
                format!("Could not parse bridge ID from name '{}'", bridge_name)
            ))?;

        let tap_id = ManageTap::get_tap_id_by_name(tap_name)
            .ok_or_else(|| BridgeError::NotFound(
                format!("Could not parse tap ID from name '{}'", tap_name)
            ))?;

        // ── 2. Attach tap to bridge in kernel ─────────────────────────────────
        let out = Command::new("sudo")
            .args(["ip", "link", "set", tap_name, "master", bridge_name])
            .output()
            .map_err(|e| BridgeError::SystemError(
                format!("ip link set master spawn failed: {}", e)
            ))?;

        if !out.status.success() {
            return Err(BridgeError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }

        // ── 3. Read operstate after attachment ────────────────────────────────
        let operstate = ManageTap::get_tap_operstate(tap_name)
            .unwrap_or(OperState::Down);

        // ── 4. Persist bridge association in DB ───────────────────────────────
        let conn = get_conn(&state.db_pool)
            .map_err(|e| BridgeError::SystemError(format!("DB connection failed: {}", e)))?;

        let rows = conn.execute(
            "UPDATE tap SET bridge_id = ?1, operstate = ?2 WHERE id = ?3",
            params![bridge_id as i64, format!("{:?}", operstate), tap_id as i64],
        ).map_err(|e| BridgeError::SystemError(format!("tap update failed: {}", e)))?;

        if rows == 0 {
            // Kernel attach succeeded but tap not in DB — detach and surface the error
            let _ = Command::new("sudo")
                .args(["ip", "link", "set", tap_name, "nomaster"])
                .output();
            return Err(BridgeError::NotFound(
                format!("Tap '{}' (id={}) not found in DB — kernel attach rolled back", tap_name, tap_id)
            ));
        }

        Ok(TapBridgeInfo {
            tap_id,
            tap_name:    tap_name.to_string(),
            bridge_id,
            bridge_name: bridge_name.to_string(),
            operstate,
        })
    }
}