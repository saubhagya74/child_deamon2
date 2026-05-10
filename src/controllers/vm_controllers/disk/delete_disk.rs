use std::process::Command;
use std::path::Path;

use crate::controllers::vm_controllers::{ManageVm, VmError};

const BASE_IMAGE:  &str = "/opt/ihk/images/wolfi-image/wolfi.qcow2";
const IMAGE_DIR:   &str = "/opt/ihk/images/wolfi-image";
const DISK_SIZE:   &str = "10G";

impl ManageVm {
    /// Deletes the qcow2 disk for a given vm_id.
    /// Call this on boot failure to clean up.
    pub fn delete_disk(vm_id: u32) -> Result<(), VmError> {
        let disk_path = format!("{}/vm{}.qcow2", IMAGE_DIR, vm_id);

        if !Path::new(&disk_path).exists() {
            return Err(VmError::SystemError(format!(
                "Disk {} does not exist", disk_path
            )));
        }

        let output = Command::new("sudo")
            .args(["rm", "-f", &disk_path])
            .output()
            .map_err(|e| VmError::SystemError(format!(
                "Failed to run rm: {}", e
            )))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(VmError::CommandFailed(format!(
                "Failed to delete disk for vm{}: {}", vm_id, stderr
            )));
        }

        Ok(())
    }
}