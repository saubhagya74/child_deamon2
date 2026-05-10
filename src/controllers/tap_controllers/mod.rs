pub mod routerfile;
pub mod create_tap;
pub mod delete_tap;
pub struct ManageTap;
#[derive(Debug)]
pub enum TapError {
    SystemError(String),
    CommandFailed(String),
    InvalidArgs(String),
    TapNotFound(String),   
}

impl std::fmt::Display for TapError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TapError::TapNotFound(s)    => write!(f, "Tap not found: {}", s),
            TapError::InvalidArgs(s)=>write!(f,"invliad args:{}",s),
            TapError::SystemError(s) => write!(f, "System Error: {}", s),
            TapError::CommandFailed(s) => write!(f, "Command Failed: {}", s),
        }
    }
}

impl ManageTap{
    pub fn get_tap_name_by_id(id: u64) -> String {
        format!("xt{}", id)
    }
    pub fn get_tap_id_by_name(tap_name:&str)->Option<u64>{
        tap_name
            .strip_prefix("xt")
            .and_then(|s| s.parse::<u64>().ok())
    }
}