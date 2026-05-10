use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};


impl ManageIp{

    /// Creates an ipset named `xis{bridge_id}` of hash:ip or hash:net.
    /// Returns the ipset name.
    pub fn create_ipset(state: &AppState, bridge_id: u64, ipset_type: &str) -> Result<String, IpError> {
        let ipset_name = format!("xis{}", bridge_id);
        let hash_type  = match ipset_type {
            "ip"  => "hash:ip",
            "net" => "hash:net",
            other => return Err(IpError::InvalidArgs(format!("Invalid ipset type: '{}'", other))),
        };
        
        let out = Command::new("sudo")
            .args(["ipset", "create", &ipset_name, hash_type])
            .output()
            .map_err(|e| IpError::SystemError(format!("ipset create spawn failed: {}", e)))?;
        
        if !out.status.success() {
            return Err(IpError::IpsetError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        
        let conn = get_conn(&state.db_pool)
        .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;
    
    conn.execute(
        "INSERT INTO ipset (id, type) VALUES (?1, ?2)",
        params![bridge_id as i64, ipset_type],
    ).map_err(|e| IpError::SystemError(format!("ipset DB insert failed: {}", e)))?;
    
    Ok(ipset_name)
}
}