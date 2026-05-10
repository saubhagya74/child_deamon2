use std::process::Command;
use serde::{Deserialize, Serialize};
use sqlsrv::rusqlite::params;
use axum::{Json, extract::State, response::IntoResponse};
use hyper::StatusCode;

use crate::{
    controllers::{
        bridge_controllers::{BridgeError, ManageBridge},
        ip_controllers::ManageIp,
        tap_controllers::ManageTap,
    },
    db_services::get_conn,
    my_states::{AppState, OperState},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeInfo {
    pub id: u64,
    pub name: String,
    pub ip: u32,
    pub cidr: i32,
    pub gateway: u32,
    pub broadcast: u32,
    pub operstate: OperState,
}

pub async fn handle_create_bridge(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match ManageBridge::create_bridge(&state) {
        Ok(info)  => (StatusCode::CREATED, Json(info)).into_response(),
        Err(e)    => (StatusCode::INTERNAL_SERVER_ERROR, Json(format!("{:?}", e))).into_response(),
    }
}

impl ManageBridge {
    pub fn create_bridge(state: &AppState) -> Result<BridgeInfo, BridgeError> {
        // ── 1. Generate ID and name (max 15 chars: "xb" + 13 digits) ─────────
        let bridge_id   = (state.id_bucket.lock().get_id() % 10_000_000_000_000) as u64;
        let bridge_name = format!("xb{}", bridge_id);
        let ipset_name  = format!("xis{}", bridge_id);
        // ── 2. Create the kernel bridge interface ────────────────────────────
        let out = Command::new("sudo")
            .args(["ip","link", "add", &bridge_name, "type", "bridge"])
            .output()
            .map_err(|e| BridgeError::SystemError(format!("ip link add spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(BridgeError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }

        // From here every error path must delete the bridge.
        // Delegate to inner fn so the del_bridge call stays in one place.
        let bridge_info = Self::allocate_and_configure(state, bridge_id, &bridge_name)
        .inspect_err(|_| {
            let _ = Command::new("sudo").args(["ip", "link", "del", &bridge_name]).output();
        })?;

        let mut ipset_created   = false;
        let mut inbound_applied = false;
        let mut outbound_applied= false;

        // network_ip = gateway - 1  (bridge_info.ip is the gateway .1)
        let network_ip = bridge_info.ip - 1;

        let net_result: Result<(), BridgeError> = (|| {
            let nic = ManageIp::get_nic_name()
                .map_err(|e| BridgeError::SystemError(format!("get_nic_name: {}", e)))?;

            // 1. Create ipset
            ManageIp::create_ipset(state, bridge_id, "net")
                .map_err(|e| BridgeError::SystemError(format!("create_ipset: {}", e)))?;
            ipset_created = true;

            // 2. Add 172.x.x.0/24 subnet to ipset
            ManageIp::add_ip_to_ipset(state, bridge_id, "net", network_ip, Some(24))
                .map_err(|e| BridgeError::SystemError(format!("add_ip_to_ipset: {}", e)))?;

            // 3. Inbound: NIC → bridge (return traffic)
            ManageIp::apply_inbound_rule(&nic, &bridge_name)
                .map_err(|e| BridgeError::SystemError(format!("apply_inbound: {}", e)))?;
            inbound_applied = true;

            // 4. Outbound: bridge → NIC (VM traffic out)
            ManageIp::apply_outbound_rule(&ipset_name, &bridge_name)
                .map_err(|e| BridgeError::SystemError(format!("apply_outbound: {}", e)))?;
            outbound_applied = true;

            // 5. SNAT: rewrite source IP to host IP
            ManageIp::apply_snat_rule(&ipset_name)
                .map_err(|e| BridgeError::SystemError(format!("apply_snat: {}", e)))?;

            Ok(())
        })();

        if let Err(e) = net_result {
            // Rollback in strict reverse order
            if outbound_applied { let _ = ManageIp::delete_outbound_rule(&ipset_name, &bridge_name); }
            if inbound_applied  {
                if let Ok(nic) = ManageIp::get_nic_name() {
                    let _ = ManageIp::delete_inbound_rule(&nic, &bridge_name);
                }
            }
            if ipset_created    { let _ = ManageIp::delete_ipset(state, bridge_id); }

            // Also undo allocate_and_configure (it already committed to DB)
            let _ = Command::new("sudo").args(["ip", "link", "del", &bridge_name]).output();
            // Clean up ip_pool + bridge rows
            if let Ok(conn) = get_conn(&state.db_pool) {
                let _ = conn.execute("DELETE FROM ip_pool WHERE dev_id = ?1", params![bridge_id as i64]);
                let _ = conn.execute("DELETE FROM bridge WHERE id = ?1",     params![bridge_id as i64]);
            }
            return Err(e);
        }

        Ok(bridge_info)
    }

    /// Allocates an IP, configures the interface, and persists everything.
    /// Called only after the bridge interface already exists.
    /// On any error the caller deletes the bridge; this fn rolls back the DB.
    fn allocate_and_configure(
        state:       &AppState,
        bridge_id:   u64,
        bridge_name: &str,
    ) -> Result<BridgeInfo, BridgeError> {

        let conn = get_conn(&state.db_pool)
            .map_err(|e| BridgeError::SystemError(format!("DB connection failed: {}", e)))?;

        // ── 3. Exclusive transaction — only one request touches ip_pool at a time
        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| BridgeError::SystemError(format!("BEGIN EXCLUSIVE failed: {}", e)))?;

        // Everything inside the closure; on Err we rollback + caller del_bridge
        let result: Result<BridgeInfo, BridgeError> = (|| {

            // ── 4. Determine next /24 network address ────────────────────────
            //
            //  Stored encoding (custom, not binary):
            //    172.A.B.C  →  A*1_000_000 + B*1_000 + C
            //
            //  ip_pool stores the *network* address (.0) of each /24 block.
            //  We read the highest one and bump the third octet (B).
            //    172.16.0.0  →  next  →  172.16.1.0  →  172.16.2.0  …

            let network_ip: u32 = match conn.query_row(
                "SELECT ip FROM ip_pool ORDER BY ip DESC LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            ) {
                Ok(last_raw) => {
                    let last = last_raw as u32;
                    let part2 = (last / 1_000_000) % 1_000; // second octet
                    let part3 = (last / 1_000)     % 1_000; // third  octet

                    let next3 = part3 + 1;
                    if next3 > 255 {
                        // Third octet overflowed → bump second octet
                        let next2 = part2 + 1;
                        if next2 > 255 {
                            return Err(BridgeError::SubnetExhausted(
                                "All 172.x.x.0/24 subnets exhausted".into(),
                            ));
                        }
                        next2 * 1_000_000           // 172.(A+1).0.0
                    } else {
                        part2 * 1_000_000 + next3 * 1_000  // 172.A.(B+1).0
                    }
                }

                // Empty table → first ever machine, start at 172.16.0.0
                Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {
                    ManageIp::ip_to_int("172.16.0.0")
                        .map_err(|e| BridgeError::SystemError(e.to_string()))?
                }

                Err(e) => return Err(BridgeError::SystemError(format!("ip_pool query failed: {}", e))),
            };

            // .0 = network, .1 = bridge/gateway, .255 = broadcast
            let gateway_ip   = network_ip + 1;
            let broadcast_ip = network_ip + 255;

            // Human-readable "172.16.0.1" for the ip command
            let ip_cidr = format!("{}/24", ManageIp::int_to_ip_display(gateway_ip));

            // ── 5. Assign IP and bring the interface up ──────────────────────
            Self::run_ip(&["addr", "add", &ip_cidr, "dev", bridge_name])?;
            Self::run_ip(&["link", "set", "dev", bridge_name, "up"])?;

            let operstate = ManageTap::get_tap_operstate(bridge_name)
                .unwrap_or(OperState::Down);

            conn.execute(
                "INSERT INTO ip_pool (ip, cidr, dev_id, slots_left, assigned_at)
                 VALUES (?1, 24, ?2, 254, strftime('%s', 'now'))",
                params![network_ip as i64, bridge_id as i64],
            ).map_err(|e| BridgeError::SystemError(format!("ip_pool insert failed: {}", e)))?;

            conn.execute(
                "INSERT INTO bridge (id, cidr, operstate) VALUES (?1, 24, ?2)",
                params![bridge_id as i64, format!("{:?}", operstate)],
            ).map_err(|e| BridgeError::SystemError(format!("bridge insert failed: {}", e)))?;

            Ok(BridgeInfo {
                id:        bridge_id,
                name:      bridge_name.to_string(),
                ip:        gateway_ip,
                cidr:      24,
                gateway:   gateway_ip,
                broadcast: broadcast_ip,
                operstate,
            })
        })();

        // ── 7. Commit or rollback ─────────────────────────────────────────────
        match result {
            Ok(info) => {
                conn.execute_batch("COMMIT").map_err(|e| {
                    let _ = conn.execute_batch("ROLLBACK");
                    BridgeError::SystemError(format!("COMMIT failed: {}", e))
                })?;
                Ok(info)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Tiny helper — runs an `ip` sub-command, maps failure to BridgeError.
    fn run_ip(args: &[&str]) -> Result<(), BridgeError> {
        let out = Command::new("sudo")
            .arg("ip")
            .args(args)
            .output()
            .map_err(|e| BridgeError::SystemError(format!("spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(BridgeError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(())
    }
}