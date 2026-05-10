// use crate::{controllers::ip_controllers::ManageIp, db_services::{get_conn}, my_states::AppState};
// use super::{IpError};
// use sqlsrv::rusqlite::params;


// impl ManageIp{
    
//     pub fn delete_ip_range(
//         state: &AppState,
//         ip: u32,
//         cidr: i32,
//     ) -> Result<(), IpError> {
//         let conn = get_conn(&state.db_pool)
//             .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;
        
//         // Delete from ip_pool
//         let deleted = conn.execute(
//             "DELETE FROM ip_pool WHERE ip = ?1 AND cidr = ?2",
//             params![ip as i64, cidr],
//         ).map_err(|e| IpError::SystemError(format!("Delete from ip_pool failed: {}", e)))?;
        
//         if deleted == 0 {
//             return Err(IpError::InvalidArgs(format!(
//                 "No IP range found for {}/{}", 
//                 Self::int_to_ip(ip), 
//                 cidr
//             )));
//         }
        
//         // Check for mergeable adjacent ranges in recycle_ip_pool
//         let current_end = ip + Self::cidr_to_size(cidr)?;
        
//         // Find range that ends right before our start
//         let merge_left = conn.query_row(
//             "SELECT ip, cidr FROM recycle_ip_pool 
//             WHERE ip + (1 << (24 - cidr)) = ?1
//             LIMIT 1",
//             params![ip as i64],
//             |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i32>(1)?)),
//         );
        
//         // Find range that starts right after our end
//         let merge_right = conn.query_row(
//             "SELECT ip, cidr FROM recycle_ip_pool 
//             WHERE ip = ?1
//             LIMIT 1",
//             params![current_end as i64],
//             |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i32>(1)?)),
//         );
        
//         match (merge_left, merge_right) {
//             (Ok((left_ip, left_cidr)), Ok((right_ip, right_cidr))) => {
//                 // Merge all three
//                 let new_size = Self::cidr_to_size(left_cidr)? + Self::cidr_to_size(cidr)? + Self::cidr_to_size(right_cidr)?;
//                 let new_cidr = Self::size_to_cidr(new_size)?;
                
//                 // Delete old entries
//                 conn.execute("DELETE FROM recycle_ip_pool WHERE ip = ?1", params![left_ip as i64])
//                     .map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
//                 conn.execute("DELETE FROM recycle_ip_pool WHERE ip = ?1", params![right_ip as i64])
//                     .map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
                
//                 // Insert merged range
//                 conn.execute(
//                     "INSERT INTO recycle_ip_pool (ip, cidr) VALUES (?1, ?2)",
//                     params![left_ip as i64, new_cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Insert recycle failed: {}", e)))?;
//             }
//             (Ok((left_ip, left_cidr)), Err(_)) => {
//                 // Merge with left only
//                 let new_size = Self::cidr_to_size(left_cidr)? + Self::cidr_to_size(cidr)?;
//                 let new_cidr = Self::size_to_cidr(new_size)?;
                
//                 conn.execute("DELETE FROM recycle_ip_pool WHERE ip = ?1", params![left_ip as i64])
//                     .map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
//                 conn.execute(
//                     "INSERT INTO recycle_ip_pool (ip, cidr) VALUES (?1, ?2)",
//                     params![left_ip as i64, new_cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Insert recycle failed: {}", e)))?;
//             }
//             (Err(_), Ok((right_ip, right_cidr))) => {
//                 // Merge with right only
//                 let new_size = Self::cidr_to_size(cidr)? + Self::cidr_to_size(right_cidr)?;
//                 let new_cidr = Self::size_to_cidr(new_size)?;
                
//                 conn.execute("DELETE FROM recycle_ip_pool WHERE ip = ?1", params![right_ip as i64])
//                     .map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
//                 conn.execute(
//                     "INSERT INTO recycle_ip_pool (ip, cidr) VALUES (?1, ?2)",
//                     params![ip as i64, new_cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Insert recycle failed: {}", e)))?;
//             }
//             (Err(_), Err(_)) => {
//                 // No merge possible, just insert
//                 conn.execute(
//                     "INSERT INTO recycle_ip_pool (ip, cidr) VALUES (?1, ?2)",
//                     params![ip as i64, cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Insert recycle failed: {}", e)))?;
//             }
//         }
        
//         Ok(())
//     }
// }