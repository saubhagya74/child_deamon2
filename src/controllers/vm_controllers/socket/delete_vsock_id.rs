use sqlsrv::rusqlite::params;
use crate::{controllers::vm_controllers::{ManageVm, VmError}, db_services::get_conn, my_states::AppState};

    /// Releases a vsock CID back into the recycle pool.
    /// Merges with adjacent ranges automatically.
    ///
    ///  Example state:  [7,9]  [16,17]
    ///  release(10)  →  [7,10] [16,17]
    ///  release(15)  →  [7,10] [15,17]
    ///  release(11)  →  [7,11] [15,17]   (extends left neighbor)
    ///  release(14)  →  [7,11] [14,17]   (extends right neighbor)
    ///  release(12)  →  [7,12] [14,17]
    ///  release(13)  →  [7,17]            (both neighbors → full merge)
impl ManageVm{

    pub fn release_vsock_id(state: &AppState, vsock_id: u32) -> Result<(), VmError> {
        let conn = get_conn(&state.db_pool)
        .map_err(|e| VmError::SystemError(format!("DB connection failed: {}", e)))?;

        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| VmError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;
            
        let result: Result<(), VmError> = (|| {
            let id = vsock_id as i64;
            
            // Helper: query one optional range row
            let find_range = |sql: &str, val: i64| -> Result<Option<(i64, i64, i64)>, VmError> {
                match conn.query_row(sql, params![val], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
                }) {
                    Ok(r)                                             => Ok(Some(r)),
                    Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(VmError::SystemError(format!("neighbor query failed: {}", e))),
                }
            };
            
            // Left neighbor:  a range whose end+1 == vsock_id
            let left = find_range(
                "SELECT rowid, beg_vsock_id, end_vsock_id
                FROM recycle_vsock_id WHERE end_vsock_id = ?1",
                id - 1,
            )?;
            
            // Right neighbor: a range whose beg-1 == vsock_id
            let right = find_range(
                "SELECT rowid, beg_vsock_id, end_vsock_id
                FROM recycle_vsock_id WHERE beg_vsock_id = ?1",
                id + 1,
            )?;
            
            match (left, right) {
                
                // ── Both neighbors → merge into one range ─────────────────────
                //   [l_beg .. id-1]  +  id  +  [id+1 .. r_end]
                //   ───────────────────────────────────────────
                //   [l_beg ............................ r_end]
                (Some((l_rowid, l_beg, _)), Some((r_rowid, _, r_end))) => {
                    conn.execute(
                        "UPDATE recycle_vsock_id SET end_vsock_id = ?1 WHERE rowid = ?2",
                        params![r_end, l_rowid],
                    ).map_err(|e| VmError::SystemError(format!("merge update failed: {}", e)))?;
                    
                    conn.execute(
                        "DELETE FROM recycle_vsock_id WHERE rowid = ?1",
                        params![r_rowid],
                    ).map_err(|e| VmError::SystemError(format!("merge delete failed: {}", e)))?;

                    let _ = l_beg; // already in the row being updated
                }

                // ── Only left neighbor → extend its end by 1 ─────────────────
                (Some((l_rowid, _, _)), None) => {
                    conn.execute(
                        "UPDATE recycle_vsock_id SET end_vsock_id = ?1 WHERE rowid = ?2",
                        params![id, l_rowid],
                    ).map_err(|e| VmError::SystemError(format!("extend end failed: {}", e)))?;
                }
                
                // ── Only right neighbor → extend its beg by -1 ───────────────
                (None, Some((r_rowid, _, _))) => {
                    conn.execute(
                        "UPDATE recycle_vsock_id SET beg_vsock_id = ?1 WHERE rowid = ?2",
                        params![id, r_rowid],
                    ).map_err(|e| VmError::SystemError(format!("extend beg failed: {}", e)))?;
                }
                
                // ── No neighbors → isolated new range ────────────────────────
                (None, None) => {
                    conn.execute(
                        "INSERT INTO recycle_vsock_id (beg_vsock_id, end_vsock_id)
                        VALUES (?1, ?1)",
                        params![id],
                    ).map_err(|e| VmError::SystemError(format!("recycle insert failed: {}", e)))?;
                }
            }
            
            Ok(())
        })();
        
        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(|e| {
                    let _ = conn.execute_batch("ROLLBACK");
                    VmError::SystemError(format!("COMMIT failed: {}", e))
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