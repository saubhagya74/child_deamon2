use serde::{Deserialize, Serialize};

pub mod routerfile;
pub mod start_vm;
pub mod socket;
pub mod disk;

#[derive(Debug,Serialize,Deserialize)]
pub enum VmError {
    SystemError(String),
    CommandFailed(String),
    InvalidArgs(String),
    VmNotFound(u32),
    SocketError(String),
    PidNotFound(u32),
    AlreadyRunning(u32),
}

impl std::fmt::Display for VmError {
    
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            VmError::SystemError(s)    => write!(f, "System error: {}", s),
            VmError::CommandFailed(s)  => write!(f, "Command failed: {}", s),
            VmError::InvalidArgs(s)    => write!(f, "Invalid args: {}", s),
            VmError::VmNotFound(id)    => write!(f, "VM {} not found", id),
            VmError::SocketError(s)    => write!(f, "Socket error: {}", s),
            VmError::PidNotFound(id)   => write!(f, "PID not found for VM {}", id),
            VmError::AlreadyRunning(id)=> write!(f, "VM {} is already running", id),
        }
    }
}
pub struct ManageVm;