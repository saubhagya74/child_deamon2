// use crate::{controllers::ip_controllers::{IpError, ManageIp, get_ip_range2::IpAssignment}, db_services::get_conn, my_states::AppState};
// use sqlsrv::rusqlite::{params};

// impl ManageIp{
//     pub fn commit_ip_range(
//         state: &AppState,
//         assignment: &IpAssignment,
//     ) -> Result<IpAssignment, IpError> {
//         let conn = get_conn(&state.db_pool)
//             .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;
        
//         let total_slots = Self::cidr_to_size(assignment.cidr)?;
//         let slots_left = total_slots - 3; // network, gateway, broadcast
        
//         match assignment.source {
//             2 => {
//                 // From recycle - already removed from recycle pool, just insert into ip_pool
//                 conn.execute(
//                     "INSERT INTO ip_pool (ip, cidr, dev_id, slots_left, assigned_at) 
//                     VALUES (?1, ?2, NULL, ?3, strftime('%s', 'now'))",
//                     params![assignment.ip as i64, assignment.cidr, slots_left as i64],
//                 ).map_err(|e| IpError::SystemError(format!("Insert ip_pool failed: {}", e)))?;
//             }
//             1 => {
//                 // New allocation
//                 conn.execute(
//                     "INSERT INTO ip_pool (ip, cidr, dev_id, slots_left, assigned_at) 
//                     VALUES (?1, ?2, NULL, ?3, strftime('%s', 'now'))",
//                     params![assignment.ip as i64, assignment.cidr, slots_left as i64],
//                 ).map_err(|e| IpError::SystemError(format!("Insert ip_pool failed: {}", e)))?;
//             }
//             _ => return Err(IpError::InvalidArgs(format!("Invalid source: {}", assignment.source))),
//         }
        
//         Ok(IpAssignment {
//             ip: assignment.ip,
//             cidr: assignment.cidr,
//             source: assignment.source,
//             gateway: assignment.gateway,
//             broadcast: assignment.broadcast,
//             total_slots,
//         })
//     }
// }