use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};
impl ManageIp{
    pub fn apply_inbound_rule(nic_name: &str, bridge_name: &str) -> Result<(), IpError> {
        let out = Command::new("sudo")
            .args([
                "iptables", "-I", "FORWARD", "2",
                "-i", nic_name, "-o", bridge_name,
                "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED",
                "-j", "ACCEPT",
            ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables inbound spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(IpError::IptablesError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        Ok(())
    }

    pub fn delete_inbound_rule(nic_name: &str, bridge_name: &str) -> Result<(), IpError> {
        let out = Command::new("sudo")
            .args([
                "iptables", "-D", "FORWARD",
                "-i", nic_name, "-o", bridge_name,
                "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED",
                "-j", "ACCEPT",
            ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables delete inbound spawn failed: {}", e)))?;

        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            if !msg.contains("No chain") && !msg.contains("does not exist") {
                return Err(IpError::IptablesError(msg));
            }
        }
        Ok(())
    }
}