use std::process::Command;

use sqlsrv::rusqlite::params;

use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};

impl ManageIp{

    pub fn apply_snat_rule(ipset_name: &str) -> Result<(), IpError> {
        let nic     = Self::get_nic_name()?;
        let gateway = Self::get_host_ip()?;
        
        let out = Command::new("sudo")
        .args([
                "iptables", "-t", "nat", "-I", "POSTROUTING", "1",
                "-m", "set", "--match-set", ipset_name, "src",
                "-o", &nic,
                "-j", "SNAT", "--to-source", &gateway,
                ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables SNAT spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(IpError::IptablesError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        Ok(())
    }
    pub fn delete_snat_rule(ipset_name: &str) -> Result<(), IpError> {
        let nic     = Self::get_nic_name()?;
        let gateway = Self::get_host_gateway()?;
        
        let out = Command::new("sudo")
        .args([
            "iptables", "-t", "nat", "-D", "POSTROUTING",
            "-m", "set", "--match-set", ipset_name, "src",
                "-o", &nic,
                "-j", "SNAT", "--to-source", &gateway,
            ])
            .output()
            .map_err(|e| IpError::SystemError(format!("iptables delete SNAT spawn failed: {}", e)))?;
        
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            if !msg.contains("No chain") && !msg.contains("does not exist") {
                return Err(IpError::IptablesError(msg));
            }
        }
        Ok(())
    }
} 