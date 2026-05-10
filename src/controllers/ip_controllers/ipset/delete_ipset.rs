use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};


impl ManageIp{

    /// Destroys the ipset and removes all DB rows. "Does not exist" is not fatal.
    pub fn delete_ipset(state: &AppState, bridge_id: u64) -> Result<(), IpError> {
        let ipset_name = format!("xis{}", bridge_id);
        
        let out = Command::new("sudo")
        .args(["ipset", "destroy", &ipset_name])
        .output()
        .map_err(|e| IpError::SystemError(format!("ipset destroy spawn failed: {}", e)))?;
    
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).to_string();
        if !msg.contains("does not exist") && !msg.contains("The set with the given name does not exist") {
            return Err(IpError::IpsetError(msg));
        }
    }
    
    let conn = get_conn(&state.db_pool)
    .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;

conn.execute("DELETE FROM ipset_ips WHERE ipset_id = ?1", params![bridge_id as i64])
.map_err(|e| IpError::SystemError(format!("ipset_ips delete failed: {}", e)))?;
conn.execute("DELETE FROM ipset WHERE id = ?1", params![bridge_id as i64])
.map_err(|e| IpError::SystemError(format!("ipset delete failed: {}", e)))?;

Ok(())
}

}