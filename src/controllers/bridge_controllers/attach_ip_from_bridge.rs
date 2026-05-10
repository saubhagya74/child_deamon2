use sqlsrv::rusqlite::params;
use crate::{
    controllers::bridge_controllers::{BridgeError, ManageBridge},
    db_services::get_conn,
    my_states::AppState,
};

impl ManageBridge {

    /// Finds a bridge with free slots and assigns the next available IP.
    /// Returns (bridge_id, assigned_ip).
    pub fn assign_bridge_ip(state: &AppState) -> Result<(u64, u32), BridgeError> {

        // ── 1. Count bridges (outside transaction — create_bridge has its own) ──
        let conn = get_conn(&state.db_pool)
            .map_err(|e| BridgeError::SystemError(format!("DB connection failed: {}", e)))?;

        let bridge_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM bridge", [],
            |row| row.get(0),
        ).map_err(|e| BridgeError::SystemError(format!("COUNT query failed: {}", e)))?;

        // ── 2. No bridges at all — create one first ───────────────────────────
        if bridge_count == 0 {
            Self::create_bridge(state)?;
            // Re-open connection after create_bridge (it may have used the pool)
            return Self::assign_bridge_ip(state);
        }

        // ── 3. Exclusive transaction for the entire IP allocation ─────────────
        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| BridgeError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;

        let result: Result<(u64, u32), BridgeError> = (|| {

            // Load all bridges ordered so fullest-first are skipped quickly
            struct BridgeRow { id: u64, slots_left: i64 }

            let bridges: Vec<BridgeRow> = {
                let mut stmt = conn.prepare(
                    "SELECT b.id, ip.slots_left
                     FROM bridge b
                     JOIN ip_pool ip ON ip.dev_id = b.id
                     ORDER BY ip.slots_left DESC",
                ).map_err(|e| BridgeError::SystemError(format!("prepare failed: {}", e)))?;

                stmt.query_map([], |row| {
                    Ok(BridgeRow {
                        id:         row.get::<_, i64>(0)? as u64,
                        slots_left: row.get::<_, i64>(1)?,
                    })
                })
                .map_err(|e| BridgeError::SystemError(format!("query failed: {}", e)))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| BridgeError::SystemError(format!("row collect failed: {}", e)))?
            };

            // ── 3.1  Loop: find first bridge with free slots ──────────────────
            for bridge in &bridges {

                // 3.2  No slots → skip
                if bridge.slots_left <= 0 {
                    continue;
                }

                let bid = bridge.id as i64;

                // 3.4  Check recycle_bridge_ip for a recycled IP on this bridge
                let recycled: Option<u32> = match conn.query_row(
                    "SELECT ip FROM recycle_bridge_ip WHERE bridge_id = ?1 LIMIT 1",
                    params![bid],
                    |row| row.get::<_, i64>(0),
                ) {
                    Ok(ip)                                            => Some(ip as u32),
                    Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => None,
                    Err(e) => return Err(BridgeError::SystemError(
                        format!("recycle query failed: {}", e)
                    )),
                };

                let assigned_ip: u32 = if let Some(ip) = recycled {
                    // 3.5 + 3.7  Found in recycle → move to bridge_ip ─────────
                    conn.execute(
                        "DELETE FROM recycle_bridge_ip WHERE bridge_id = ?1 AND ip = ?2",
                        params![bid, ip as i64],
                    ).map_err(|e| BridgeError::SystemError(format!("recycle delete failed: {}", e)))?;

                    conn.execute(
                        "INSERT INTO bridge_ip (bridge_id, ip) VALUES (?1, ?2)",
                        params![bid, ip as i64],
                    ).map_err(|e| BridgeError::SystemError(format!("bridge_ip insert failed: {}", e)))?;

                    ip

                } else {
                    // 3.6 + 3.8  Not in recycle → derive next IP ──────────────

                    // Network address for this bridge (.0)
                    let network_ip: u32 = conn.query_row(
                        "SELECT ip FROM ip_pool WHERE dev_id = ?1",
                        params![bid],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|v| v as u32)
                    .map_err(|e| BridgeError::SystemError(
                        format!("ip_pool lookup failed: {}", e)
                    ))?;

                    // Highest currently assigned IP on this bridge
                    let next_ip: u32 = match conn.query_row(
                        "SELECT MAX(ip) FROM bridge_ip WHERE bridge_id = ?1",
                        params![bid],
                        |row| row.get::<_, Option<i64>>(0),
                    ) {
                        Ok(Some(max_ip)) => max_ip as u32 + 1,
                        Ok(None)         => network_ip + 2, // first device: skip .0 (net) and .1 (gw)
                        Err(e) => return Err(BridgeError::SystemError(
                            format!("MAX(ip) query failed: {}", e)
                        )),
                    };

                    // Guard: last octet must stay ≤ 254 (.255 is broadcast)
                    if next_ip % 1_000 > 254 {
                        continue; // bridge_ip exhausted despite slots_left — try next bridge
                    }

                    conn.execute(
                        "INSERT INTO bridge_ip (bridge_id, ip) VALUES (?1, ?2)",
                        params![bid, next_ip as i64],
                    ).map_err(|e| BridgeError::SystemError(format!("bridge_ip insert failed: {}", e)))?;

                    next_ip
                };

                // Consume one slot
                conn.execute(
                    "UPDATE ip_pool SET slots_left = slots_left - 1 WHERE dev_id = ?1",
                    params![bid],
                ).map_err(|e| BridgeError::SystemError(format!("slots_left update failed: {}", e)))?;

                return Ok((bridge.id, assigned_ip));
            }

            Err(BridgeError::SubnetExhausted(
                "All bridges are full — create a new bridge first".into()
            ))
        })();

        // ── Commit or rollback ────────────────────────────────────────────────
        match result {
            Ok(pair) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| {
                        let _ = conn.execute_batch("ROLLBACK");
                        BridgeError::SystemError(format!("COMMIT failed: {}", e))
                    })?;
                Ok(pair)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────

}