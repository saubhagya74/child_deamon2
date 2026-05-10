use sqlsrv::rusqlite::params;
use crate::{
    controllers::bridge_controllers::{BridgeError, ManageBridge},
    db_services::get_conn,
    my_states::AppState,
};
/// Releases an IP back to the recycle pool for the given bridge.
impl ManageBridge{
    pub fn detach_ip_got_from_bridge(
        state:     &AppState,
        bridge_id: u64,
        ip:        u32,
    ) -> Result<(), BridgeError> {

        let conn = get_conn(&state.db_pool)
            .map_err(|e| BridgeError::SystemError(format!("DB connection failed: {}", e)))?;

        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| BridgeError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;

        let result: Result<(), BridgeError> = (|| {
            
            let bid = bridge_id as i64;

            let owned: bool = conn.query_row(
                "SELECT COUNT(*) FROM bridge_ip WHERE bridge_id = ?1 AND ip = ?2",
                params![bid, ip as i64],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .map_err(|e| BridgeError::SystemError(format!("ownership check failed: {}", e)))?;

            if !owned {
                return Err(BridgeError::NotFound(
                    format!("IP {} not assigned to bridge {}", ip, bridge_id)
                ));
            }

            // Move: bridge_ip → recycle_bridge_ip
            conn.execute(
                "DELETE FROM bridge_ip WHERE bridge_id = ?1 AND ip = ?2",
                params![bid, ip as i64],
            ).map_err(|e| BridgeError::SystemError(format!("bridge_ip delete failed: {}", e)))?;

            conn.execute(
                "INSERT INTO recycle_bridge_ip (bridge_id, ip) VALUES (?1, ?2)",
                params![bid, ip as i64],
            ).map_err(|e| BridgeError::SystemError(format!("recycle insert failed: {}", e)))?;

            // Free the slot
            conn.execute(
                "UPDATE ip_pool SET slots_left = slots_left + 1 WHERE dev_id = ?1",
                params![bid],
            ).map_err(|e| BridgeError::SystemError(format!("slots_left update failed: {}", e)))?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| {
                        let _ = conn.execute_batch("ROLLBACK");
                        BridgeError::SystemError(format!("COMMIT failed: {}", e))
                    })?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}