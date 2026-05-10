use std::process::Command;

use serde::{Deserialize, Serialize};

pub mod routerfile;
pub mod create_bridge;
pub mod delete_bridge;
pub mod attach_ip_from_bridge;
pub mod detach_ip_got_from_bridge;
pub mod attach_tap_to_bridge;
pub struct ManageBridge;
#[derive(Debug,Serialize,Deserialize)]
pub enum BridgeError {
    SystemError(String),
    CommandFailed(String),
    InvalidArgs(String),
    TapNotFound(String),   
    BridgeNotFound(String), 
    SubnetExhausted(String),
    NotFound(String)
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BridgeError::InvalidArgs(s)=>write!(f,"invliad args:{}",s),
            BridgeError::SystemError(s) => write!(f, "System Error: {}", s),
            BridgeError::CommandFailed(s) => write!(f, "Command Failed: {}", s),
            BridgeError::TapNotFound(s)    => write!(f, "Tap not found: {}", s),
            BridgeError::BridgeNotFound(s) => write!(f, "Bridge not found: {}", s),
            BridgeError::SubnetExhausted(s) => write!(f, "Subnet exhausted: {}", s),
            BridgeError::NotFound(s) => write!(f, "bridge not found : {}", s),
        }
    }
}

impl ManageBridge{
    pub fn get_bridge_name_by_id(id: u64) -> String {
        format!("xb{}", id)
    }
    pub fn get_bridge_id_by_name(bridge_name:&str)->Option<u64>{
        bridge_name
            .strip_prefix("xb")
            .and_then(|s| s.parse::<u64>().ok())
    }
    pub fn get_bridge_gateway_ip(bridge_name: &str) -> Result<String, BridgeError> {
        let output = Command::new("ip")
            .args(["-4", "addr", "show", "dev", bridge_name])
            .output()
            .map_err(|e| BridgeError::SystemError(
                format!("ip addr show spawn failed: {}", e)
            ))?;

        if !output.status.success() {
            return Err(BridgeError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Output looks like:
        //   3: xb123: <BROADCAST,MULTICAST,UP> ...
        //       inet 172.16.0.1/24 brd 172.16.0.255 scope global xb123
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("inet ") {
                // ["inet", "172.16.0.1/24", "brd", ...]
                if let Some(ip_cidr) = trimmed.split_whitespace().nth(1) {
                    // Strip "/24"
                    let ip = ip_cidr.split('/').next()
                        .ok_or_else(|| BridgeError::SystemError(
                            format!("Malformed inet line on {}: '{}'", bridge_name, trimmed)
                        ))?;
                    return Ok(ip.to_string());
                }
            }
        }

        Err(BridgeError::SystemError(
            format!("No IPv4 address found on bridge '{}'", bridge_name)
        ))
    }
}