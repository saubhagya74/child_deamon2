use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};


impl ManageIp{

    /// Adds an IP (or network CIDR) to the ipset and records it in DB.
    pub fn add_ip_to_ipset(
        state:      &AppState,
        bridge_id:  u64,
        ipset_type: &str,
        ip:         u32,
        cidr:       Option<i32>,
    ) -> Result<(), IpError> {
        let ipset_name = format!("xis{}", bridge_id);
        let entry = match ipset_type {
            "net" => {
                let c = cidr.ok_or_else(|| IpError::InvalidArgs(
                    "CIDR is required for 'net' type ipset".into()
                ))?;
                format!("{}/{}", Self::int_to_ip_display(ip), c)
            }
            "ip" => Self::int_to_ip_display(ip),
            other => return Err(IpError::InvalidArgs(format!("Invalid type: '{}'", other))),
        };
        
        let out = Command::new("sudo")
        .args(["ipset", "add", &ipset_name, &entry])
        .output()
        .map_err(|e| IpError::SystemError(format!("ipset add spawn failed: {}", e)))?;
    
    if !out.status.success() {
        return Err(IpError::IpsetError(
            String::from_utf8_lossy(&out.stderr).to_string()
        ));
    }
    
    let conn = get_conn(&state.db_pool)
    .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;

        conn.execute(
            "INSERT INTO ipset_ips (ipset_id, ip) VALUES (?1, ?2)",
            params![bridge_id as i64, ip as i64],
        ).map_err(|e| IpError::SystemError(format!("ipset_ips DB insert failed: {}", e)))?;

        Ok(())
    }
}