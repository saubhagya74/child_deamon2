use std::process::Command;
use std::path::Path;

use crate::controllers::vm_controllers::{ManageVm, VmError};

const BASE_IMAGE:  &str = "/opt/ihk/images/wolfi-image/wolfi.qcow2";
const IMAGE_DIR:   &str = "/opt/ihk/images/wolfi-image";
const DISK_SIZE:   &str = "10G";

impl ManageVm {
    /// Creates a qcow2 disk backed by the base image.
    /// Uses vm_id for naming: vm1.qcow2, vm2.qcow2 opt.
    /// Returns the disk path on success.
    pub fn create_disk(disk_path: &str) -> Result<(), VmError> {
        let disk_out = Command::new("sudo")
            .args(["qemu-img", "create", "-f", "qcow2", "-F", "qcow2",
                "-b", BASE_IMAGE, disk_path, DISK_SIZE])
            .output()
            .map_err(|e| VmError::SystemError(format!("qemu-img spawn: {}", e)))?;
        
        if !disk_out.status.success() {
            return Err(VmError::CommandFailed(format!("qemu-img: {}",
                String::from_utf8_lossy(&disk_out.stderr))));
        }
        
        let user = std::env::var("USER").unwrap_or_else(|_| "saubhagya".to_string());
        let chown_out = Command::new("sudo")
            .args(["chown", &format!("{}:{}", user, user), disk_path])
            .output()
            .map_err(|e| VmError::SystemError(format!("chown failed: {}", e)))?;
        
        if !chown_out.status.success() {
            return Err(VmError::CommandFailed(format!("chown: {}",
                String::from_utf8_lossy(&chown_out.stderr))));
        }
        
        Ok(())
    }
}