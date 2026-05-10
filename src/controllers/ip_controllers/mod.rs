use std::{net::Ipv4Addr, process::Command};

use eui48::MacAddress;

pub mod commit_ip_range;
pub mod get_ip_range2;
pub mod routerfile;
pub mod delete_ip_range;
pub mod ipset;
pub mod iptables;

#[derive(Debug)]
pub enum IpError {
    SystemError(String),
    CommandFailed(String),
    InvalidArgs(String),
    GatewayNotFound,
    IpParseError(String),
    RouteError(String),
    AddressExhausted,
    IptablesError(String),
    IpsetError(String),
}

impl std::fmt::Display for IpError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            IpError::SystemError(s)     => write!(f, "System error: {}", s),
            IpError::CommandFailed(s)   => write!(f, "Command failed: {}", s),
            IpError::InvalidArgs(s)     => write!(f, "Invalid args: {}", s),
            IpError::GatewayNotFound    => write!(f, "Gateway not found"),
            IpError::IpParseError(s)    => write!(f, "IP parse error: {}", s),
            IpError::RouteError(s)      => write!(f, "Route error: {}", s),
            IpError::AddressExhausted   => write!(f, "IP address space exhausted"),
            IpError::IptablesError(s)   => write!(f, "Iptables error: {}", s),
            IpError::IpsetError(s)      => write!(f, "Ipset error: {}", s),
        }
    }
}
pub struct ManageIp;
impl ManageIp{
    
    pub fn int_to_ip_display(num: u32) -> String {
        let part2 = (num / 1_000_000) % 1_000;
        let part3 = (num / 1_000) % 1_000;
        let part4 = num % 1_000;
        format!("172.{}.{}.{}", part2, part3, part4)
    }
    pub fn int_to_ip(num: u32) -> String {
        let part2 = (num / 1_000_000) % 1_000;
        let part3 = (num / 1_000) % 1_000;
        let part4 = num % 1_000;
        format!("172.{:03}.{:03}.{:03}", part2, part3, part4)
    }

    pub fn ip_to_int(ip: &str) -> Result<u32, IpError> {
        let parts: Vec<&str> = ip.split('.').collect();
        
        if parts.len() != 4 {
            return Err(IpError::IpParseError(format!("Invalid IP format: {}", ip)));
        }
        
        if parts[0] != "172" {
            return Err(IpError::IpParseError(format!("Only 172.x.x.x supported, got: {}", ip)));
        }
        
        let part2: u32 = parts[1].parse()
            .map_err(|_| IpError::IpParseError(format!("Invalid octet: {}", parts[1])))?;
        let part3: u32 = parts[2].parse()
            .map_err(|_| IpError::IpParseError(format!("Invalid octet: {}", parts[2])))?;
        let part4: u32 = parts[3].parse()
            .map_err(|_| IpError::IpParseError(format!("Invalid octet: {}", parts[3])))?;
        
        // Validate octet ranges
        if part2 > 255 || part3 > 255 || part4 > 255 {
            return Err(IpError::IpParseError(
                format!("Octets must be 0-255, got: {}.{}.{}", part2, part3, part4)
            ));
        }
        
        let encoded = part2 * 1_000_000 + part3 * 1_000 + part4;
        Ok(encoded)
    }

    pub fn size_to_cidr(size: u32) -> Result<i32, IpError> {
            let host_bits = (size as f64).log2() as i32;
            Ok(24 - host_bits)
        }
    pub fn cidr_to_size(cidr: i32) -> Result<u32, IpError> {
        if cidr < 0 || cidr > 32 {
            return Err(IpError::InvalidArgs(format!("Invalid CIDR: /{}", cidr)));
        }
        Ok(1_u32 << (32 - cidr as u32)) // /24 → 2^8 = 256, /30 → 2^2 = 4
    }
    pub fn get_subnet_mask(cidr: i32) -> Result<u32, IpError> {
        let size = Self::cidr_to_size(cidr)?;
        Ok(size - 1) // mask for the encodable part
    }

    pub fn calculate_broadcast(network: u32, cidr: i32) -> Result<u32, IpError> {
        let size = Self::cidr_to_size(cidr)?;
        Ok(network + size - 1)
    }
    pub fn get_mac_from_ipv4(ip: &Ipv4Addr) -> Result<String, IpError> {
    let octets = ip.octets(); // [172, 16, 0, 2]

    let bytes: [u8; 6] = [
        0x06,       // locally administered, unicast
        0x00,       // padding
        octets[0],  // 172 → ac
        octets[1],  // 16  → 10
        octets[2],  // 0   → 00
        octets[3],  // 2   → 02
    ];

    // Format as uppercase hex with colons: "06:00:AC:10:00:02"
    Ok(format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
    ))
}   
    pub fn get_host_gateway() -> Result<String, IpError> {
        let out = Command::new("sh")
            .args(["-c", "ip route show default | awk '/default/ {print $3}'"])
            .output()
            .map_err(|e| IpError::SystemError(format!("gateway spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(IpError::RouteError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        let gw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if gw.is_empty() { return Err(IpError::GatewayNotFound); }
        Ok(gw)
    }
    /// e.g. the IP assigned to wlp0s20f3, not the router's IP.
    pub fn get_host_ip() -> Result<String, IpError> {
        // `ip route get` asks the kernel "what source IP would I use to reach 1.1.1.1?"
        // This reliably returns the host's own outbound IP regardless of interface name.
        let out = Command::new("sh")
            .args(["-c", "ip route get 1.1.1.1 | awk '/src/{for(i=1;i<=NF;i++) if($i==\"src\") print $(i+1)}'"])
            .output()
            .map_err(|e| IpError::SystemError(format!("host IP spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(IpError::RouteError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if ip.is_empty() {
            return Err(IpError::RouteError("Could not determine host IP".into()));
        }
        Ok(ip)
    }
    pub fn get_nic_name() -> Result<String, IpError> {
        // 5th field of `ip route show default` is the outbound interface
        let out = Command::new("sh")
            .args(["-c", "ip route show default | awk '/default/ {print $5}'"])
            .output()
            .map_err(|e| IpError::SystemError(format!("nic spawn failed: {}", e)))?;

        if !out.status.success() {
            return Err(IpError::RouteError(
                String::from_utf8_lossy(&out.stderr).to_string()
            ));
        }
        let nic = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if nic.is_empty() {
            return Err(IpError::RouteError("No default NIC found".into()));
        }
        Ok(nic)
    }

}