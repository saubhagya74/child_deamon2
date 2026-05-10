use std::{mem, path::Path, process::{Command, Stdio}, thread, time::Duration};
use axum::{Json, extract::State, response::IntoResponse};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlsrv::rusqlite::params;
use crate::{
    controllers::{
        bridge_controllers::ManageBridge,
        tap_controllers::ManageTap,
        ip_controllers::ManageIp,
        vm_controllers::{VmError, ManageVm},
    },
    db_services::get_conn,
    my_states::AppState,
};

const CH_BIN:         &str = "/opt/ihk/cloud-hypervisor/cloud-hypervisor-static";
const CH_REMOTE:      &str = "/opt/ihk/cloud-hypervisor/ch-remote-static";
const IMAGE_DIR:      &str = "/opt/ihk/images/wolfi-image";
const DEFAULT_KERNEL: &str = "/opt/ihk/cloud-hypervisor/linux/vmlinux";
const SOCKET_WAIT_MS: u64  = 100;
const SOCKET_WAIT_TRIES: u32 = 50; // 5 seconds total

#[derive(Serialize,Deserialize)]
pub struct StartVmInput {
    pub boot_vcpus:  u8,
    pub max_vcpus:   u8,
    pub memory_size: u64,
    pub vm_id: Option<u32>
}

pub async fn handle_start_vm(
    State(state): State<AppState>,
    Json(input): Json<StartVmInput>,
) -> impl IntoResponse {

    match ManageVm::start_vm_inner(&state, input).await {
        Ok(info) => (StatusCode::CREATED,            Json(info)).into_response(),
        Err(e)   => (StatusCode::INTERNAL_SERVER_ERROR, Json(format!("{:?}", e))).into_response(),
    }
}
impl ManageVm{

    async fn start_vm_inner(state: &AppState, input: StartVmInput) -> Result<Value, VmError> {
        
        let mut tap_created    = false;
        let mut ip_assigned    = false;
        let mut tap_attached   = false;
        let mut vsock_acquired = false;
        let mut disk_created   = false;
        let mut ch_spawned     = false;
        
        let mut rb_tap_name  = String::new();
        let mut rb_bridge_id = 0u64;
        let mut rb_ip        = 0u32;
        let mut rb_vsock     = 0u32;
        let mut rb_vm_id     = 0u64;
        let mut rb_pid       = 0u32;
        
        let result: Result<Value, VmError> = async {
            
            let vm_id: u64 = match input.vm_id {
                Some(id) => id as u64,
                None     => (state.id_bucket.lock().get_id() % 10_000_000_000_000) as u64,
            };
            rb_vm_id = vm_id;
            
            let tap = ManageTap::create_tap(state).await
            .map_err(|e| VmError::SystemError(format!("create_tap: {}", e)))?;
        
            let tap_name = ManageTap::get_tap_name_by_id(tap.id);
            tap_created  = true;
            rb_tap_name  = tap_name.clone();
            
            //assign ip from bridge having free slots.
            let (bridge_id, assigned_ip_int) = ManageBridge::assign_bridge_ip(state)
            .map_err(|e| VmError::SystemError(format!("assign_bridge_ip: {}", e)))?;

            ip_assigned  = true;
            rb_bridge_id = bridge_id;
            rb_ip        = assigned_ip_int;

            // ── 4. Human-readable IP ("172.16.0.2") ──────────────────────────────
            let vm_ip_str = ManageIp::int_to_ip_display(assigned_ip_int);

            // ── 5. Attach tap → bridge ────────────────────────────────────────────
            let bridge_name  = ManageBridge::get_bridge_name_by_id(bridge_id);
            let tap_bridge   = ManageBridge::attach_tap_to_bridge(state, &tap_name, &bridge_name)
            .map_err(|e| VmError::SystemError(format!("attach_tap_to_bridge: {}", e)))?;
            tap_attached = true;

            // ── 6. Vsock CID ──────────────────────────────────────────────────────
            let vsock_cid  = ManageVm::get_vsock_id(state)?;
            vsock_acquired = true;
            rb_vsock       = vsock_cid;

            // ── 7. Gateway IP (from running bridge interface) ─────────────────────
            let gateway_ip = ManageBridge::get_bridge_gateway_ip(&bridge_name)
                .map_err(|e| VmError::SystemError(format!("get_bridge_gateway_ip: {}", e)))?;
            
            // ── 8. MAC from VM IP ─────────────────────────────────────────────────
            let vm_ipv4: std::net::Ipv4Addr = vm_ip_str.parse()
                .map_err(|_| VmError::SystemError(format!("IP parse failed: {}", vm_ip_str)))?;
            let mac_str = ManageIp::get_mac_from_ipv4(&vm_ipv4)
                .map_err(|e| VmError::SystemError(format!("get_mac_from_ipv4: {}", e)))?;
            // ── 9. Disk ───────────────────────────────────────────────────────────
            let disk_path = format!("{}/vm{}.qcow2", IMAGE_DIR, vm_id);
            if !input.vm_id.is_some() {
                ManageVm::create_disk(&disk_path)?;
                disk_created = true;
            }
            
            // ── 10. Socket + vsock paths ──────────────────────────────────────────
            let socket_path = format!("/tmp/vm{}.sock",   vm_id);
            let vsock_path  = format!("/tmp/vm{}.vsock",  vm_id);
            
            // ── 11. Spawn cloud-hypervisor ────────────────────────────────────────
            //
            // SOCKET MEMORY SAFETY:
            //   - stdio is all null so our process holds zero fd references to the VM
            //   - mem::forget detaches the Child handle — no Rust wait(), no zombie
            //   - orphaned child is reparented to init/systemd which reaps on exit
            //   - process_group(0) isolates it from our terminal signals
            //
            let child = Command::new("sudo")
            .args([CH_BIN, "--api-socket", &format!("path={}", socket_path)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| VmError::SystemError(format!("CH spawn failed: {}", e)))?;
            
            rb_pid     = child.id();
            ch_spawned = true;
            mem::forget(child); // ← detach; init reaps when VM eventually exits
            
            // Wait for API socket to appear (max 5 s)
            let mut ready = false;
            for _ in 0..SOCKET_WAIT_TRIES {
                if Path::new(&socket_path).exists() { ready = true; break; }
                thread::sleep(Duration::from_millis(SOCKET_WAIT_MS));
            }
            if !ready {
                return Err(VmError::SystemError(
                    format!("API socket '{}' never appeared (5 s timeout)", socket_path)
                ));
            }

            // ── 12. VM config ─────────────────────────────────────────────────────
            // cmdline carries static IP config so no DHCP needed inside the VM
            let cmdline = format!(
                "console=ttyS0 root=/dev/vda rw init=/usr/sbin/openrc-init \
                ip={}::{}:255.255.255.0::eth0:off",
                vm_ip_str, gateway_ip
            );
            let memory_bytes = input.memory_size * 1024 * 1024;
            
            let vm_config = json!({
                "cpus":    { "boot_vcpus": input.boot_vcpus, "max_vcpus": input.max_vcpus },
                "memory":  { "size": memory_bytes, "hotplug_method": "VirtioMem", "mergeable": true },
                "payload": { "kernel": DEFAULT_KERNEL, "cmdline": cmdline },
                "disks":   [{ "path": disk_path, "backing_files": true, "image_type": "Qcow2" }],  // ← "backing_files": true
                "net":     [{ "tap": tap_name, "mac": mac_str }],
                "vsock":   { "cid": vsock_cid, "socket": vsock_path },
                "console": { "mode": "Off" },
                "serial":  { "mode": "Tty" },
                "balloon": { "size": 0, "deflate_on_oom": true, "free_page_reporting": true }
            });
            
            // ── 13. PUT /api/v1/vm.create ─────────────────────────────────────────
            //
            // SOCKET MEMORY SAFETY:
            //   --no-keepalive + Connection:close forces the API server to close
            //   the TCP-over-Unix-socket session immediately after each response.
            //   Without this the server-side buffers accumulate per-request and
            //   RAM climbs into the hundreds of MB under repeated calls.
            //
            // ── 13. Create VM with config (PUT to vm.create) ─────────────────────────
            eprintln!("Sending config: {}", vm_config);
            let create_out = Command::new("sudo")
            .args([
                "curl",
                "--unix-socket", &socket_path,
                "--no-keepalive",
                "-H", "Connection: close",
                "-X", "PUT","-v",
                "http://localhost/api/v1/vm.create",
                "-H", "Content-Type: application/json",
                "-d", &vm_config.to_string(),
                "--max-time", "30",
                "--fail",  // ← ADD THIS: makes curl return non-zero on HTTP errors
                "--show-error",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| VmError::SystemError(format!("curl vm.create spawn: {}", e)))?;

            if !create_out.status.success() {
                let error_body = String::from_utf8_lossy(&create_out.stdout);
                return Err(VmError::CommandFailed(format!(
                    "vm.create failed (HTTP error): stdout: {}, stderr: {}",
                    error_body,
                    String::from_utf8_lossy(&create_out.stderr)
                )));
            }

            // Verify config was stored - get it back
            let verify_out = Command::new("sudo")
            .args([
                "curl",
                "--unix-socket", &socket_path,
                "--no-keepalive",
                "-H", "Connection: close",
                "-X", "GET",
                "http://localhost/api/v1/vm.info",
                "--max-time", "10",
                "--fail",
                "--silent",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| VmError::SystemError(format!("curl vm.info spawn: {}", e)))?;

            if !verify_out.status.success() {
                return Err(VmError::CommandFailed(format!(
                    "vm.info failed - config not stored: {}",
                    String::from_utf8_lossy(&verify_out.stderr)
                )));
            }

            // Optional: Check response contains expected VM data
            let info_response = String::from_utf8_lossy(&verify_out.stdout);
            if info_response.contains("error") || info_response.contains("missing") {
                return Err(VmError::CommandFailed(format!(
                    "VM config not properly stored: {}", info_response
                )));
            }

            // ── 14. Boot ──────────────────────────────────────────────────────────
            let boot_out = Command::new("sudo")
            .args([CH_REMOTE, "--api-socket", &socket_path, "boot"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| VmError::SystemError(format!("ch-remote boot spawn: {}", e)))?;

            if !boot_out.status.success() {
                let boot_error = String::from_utf8_lossy(&boot_out.stderr);
                return Err(VmError::CommandFailed(format!(
                    "boot: {}", boot_error
                )));
            }

            // ── 15. Persist VM row ────────────────────────────────────────────────
            let conn = get_conn(&state.db_pool)
            .map_err(|e| VmError::SystemError(format!("DB connection failed: {}", e)))?;
        
            conn.execute(
                "INSERT INTO vm (id, ip, tap_id, bridge_id, disk_path, pid, status, memory_mb)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7)",
                params![
                    vm_id as i64,
                    &vm_ip_str,
                    tap.id as i64,
                    bridge_id as i64,
                    &disk_path,
                        rb_pid as i64,
                        input.memory_size as i64,
                        ],
                    ).map_err(|e| VmError::SystemError(format!("vm insert: {}", e)))?;
                
                // ── 16. Rich response (no extra queries — built from what we already have)
                Ok(json!({
                    "vm": {
                        "id":        vm_id,
                        "status":    "running",
                        "pid":       rb_pid,
                        "memory_mb": input.memory_size,
                        "vcpus":     { "boot": input.boot_vcpus, "max": input.max_vcpus }
                    },
                    "network": {
                        "ip":          vm_ip_str,
                        "gateway":     gateway_ip,
                        "subnet_mask": "255.255.255.0",
                        "cidr":        24,
                        "mac":         mac_str,
                    "bridge_id":   bridge_id,
                    "bridge_name": bridge_name
                },
                "tap": {
                    "id":        tap.id,
                    "name":      tap_name,
                    "bridge_id": bridge_id,
                    "operstate": format!("{:?}", tap_bridge.operstate)
                },
                "vsock": {
                    "cid":    vsock_cid,
                    "socket": vsock_path
                },
                "disk": {
                    "path":       disk_path,
                    "pre_existed": input.vm_id.is_some()
                },
                "socket": {
                    "api_socket": socket_path
                }
            }))
        }.await;

        // ── Rollback — reverse order of creation ──────────────────────────────────
        if let Err(ref _e) = result {
            
            // Kill CH process first so it releases the socket/vsock files
            if ch_spawned {
                let _ = Command::new("sudo")
                .args(["kill", "-9", &rb_pid.to_string()])
                .output();
            // Small grace period for the process to fully exit
            thread::sleep(Duration::from_millis(200));
            let _ = std::fs::remove_file(format!("/tmp/vm{}.sock",  rb_vm_id));
            let _ = std::fs::remove_file(format!("/tmp/vm{}.vsock", rb_vm_id));
            }
            
            if disk_created {
                let _ = ManageVm::delete_disk(rb_vm_id as u32);
            }
            
            if vsock_acquired {
                let _ = ManageVm::release_vsock_id(state, rb_vsock);
            }
            
            if tap_attached {
                let _ = Command::new("sudo")
                .args(["ip", "link", "set", &rb_tap_name, "nomaster"])
                    .output();
                // let _ = ManageTap::delete_tap(state, rb_tap_name.clone()).await;
            }
            
            if ip_assigned {
                let _ = ManageBridge::detach_ip_got_from_bridge(state, rb_bridge_id, rb_ip);
            }
            
            if tap_created {
                let _ = ManageTap::delete_tap(state, rb_tap_name.clone()).await;
            }
        }

        result
    }
}