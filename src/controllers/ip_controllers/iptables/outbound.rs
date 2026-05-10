use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};

impl ManageIp{

    pub fn apply_outbound_rule(ipset_name: &str, bridge_name: &str) -> Result<(), IpError> {
        let nic = Self::get_nic_name()?;
        
        let out = Command::new("sudo")
        .args([
            "iptables", "-I", "FORWARD", "1",
            "-i", bridge_name, "-o", &nic,
            "-m", "set", "--match-set", ipset_name, "src",
            "-j", "ACCEPT",
            ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables outbound spawn failed: {}", e)))?;
        
        if !out.status.success() {
            return Err(IpError::IptablesError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        Ok(())
    }
    
    pub fn delete_outbound_rule(ipset_name: &str, bridge_name: &str) -> Result<(), IpError> {
        let nic = Self::get_nic_name()?;
        
        let out = Command::new("sudo")
        .args([
            "iptables", "-D", "FORWARD",
            "-i", bridge_name, "-o", &nic,
            "-m", "set", "--match-set", ipset_name, "src",
            "-j", "ACCEPT",
            ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables delete outbound spawn failed: {}", e)))?;
        
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            if !msg.contains("No chain") && !msg.contains("does not exist") {
                return Err(IpError::IptablesError(msg));
            }
        }
        Ok(())
    }
}