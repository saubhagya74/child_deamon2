use sqlsrv::rusqlite::params;
use crate::{controllers::vm_controllers::{ManageVm, VmError}, db_services::get_conn, my_states::AppState};

impl ManageVm {

    /// Returns the next available vsock CID.
    /// Takes from recycle pool first; otherwise increments the global max.
    pub fn get_vsock_id(state: &AppState) -> Result<u32, VmError> {
        let conn = get_conn(&state.db_pool)
            .map_err(|e| VmError::SystemError(format!("DB connection failed: {}", e)))?;

        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| VmError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;

        let result: Result<u32, VmError> = (|| {

            // ── 1. Check recycle pool — take the minimum beg ─────────────────
            match conn.query_row(
                "SELECT rowid, beg_vsock_id, end_vsock_id
                 FROM recycle_vsock_id
                 ORDER BY beg_vsock_id ASC
                 LIMIT 1",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,  // rowid
                    row.get::<_, i64>(1)?,  // beg
                    row.get::<_, i64>(2)?,  // end
                )),
            ) {
                Ok((rowid, beg, end)) => {
                    // Take beg; shrink or delete the range
                    if beg == end {
                        conn.execute(
                            "DELETE FROM recycle_vsock_id WHERE rowid = ?1",
                            params![rowid],
                        ).map_err(|e| VmError::SystemError(format!("recycle delete failed: {}", e)))?;
                    } else {
                        conn.execute(
                            "UPDATE recycle_vsock_id SET beg_vsock_id = ?1 WHERE rowid = ?2",
                            params![beg + 1, rowid],
                        ).map_err(|e| VmError::SystemError(format!("recycle shrink failed: {}", e)))?;
                    }
                    return Ok(beg as u32);
                }

                // ── 2. Recycle empty — use/increment global max ───────────────
                Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => return Err(VmError::SystemError(format!("recycle query failed: {}", e))),
            }

            let new_id: u32 = match conn.query_row(
                "SELECT id FROM vsock_id LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            ) {
                Ok(current_max) => {
                    let next = current_max as u32 + 1;
                    conn.execute(
                        "UPDATE vsock_id SET id = ?1",
                        params![next as i64],
                    ).map_err(|e| VmError::SystemError(format!("vsock_id update failed: {}", e)))?;
                    next
                }

                // First ever vsock — CID 0 = host, 1 = hypervisor, 2 = reserved → start at 3
                Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {
                    conn.execute(
                        "INSERT INTO vsock_id (id) VALUES (3)",
                        [],
                    ).map_err(|e| VmError::SystemError(format!("vsock_id init failed: {}", e)))?;
                    3
                }

                Err(e) => return Err(VmError::SystemError(format!("vsock_id query failed: {}", e))),
            };

            Ok(new_id)
        })();

        match result {
            Ok(id) => {
                conn.execute_batch("COMMIT").map_err(|e| {
                    let _ = conn.execute_batch("ROLLBACK");
                    VmError::SystemError(format!("COMMIT failed: {}", e))
                })?;
                Ok(id)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
}