// use axum::{Json, extract::State, response::IntoResponse};
// use hyper::StatusCode;
// use serde::{Deserialize, Serialize};
// use sqlsrv::rusqlite::{Connection, params};

// use crate::{controllers::ip_controllers::{IpError, ManageIp}, db_services::get_conn, my_states::AppState};

// #[derive(Deserialize,Serialize)]
// pub struct IpAssignment {
//     pub ip: u32,
//     pub cidr: i32,
//     pub source: i32, // 1 = from ip_pool, 2 = from recycle
//     pub gateway: u32,
//     pub broadcast: u32,
//     pub total_slots: u32,
// }
// #[derive(Deserialize,Serialize)]
// pub struct IpRequest {
//     pub cidr: Option<i32>,
// }

// pub async fn handle_get_ip(
//     State(state): State<AppState>,
//     Json(payload): Json<IpRequest>,
// ) -> impl IntoResponse {
//     match ManageIp::get_ip(&state, payload.cidr) {
//         // Wrap 'assignment' in Json(...)
//         Ok(assignment) => (StatusCode::OK, Json(assignment)).into_response(),
        
//         Err(e) => (
//             StatusCode::BAD_REQUEST, 
//             Json(format!("IP Allocation failed: {:?}", e))
//         ).into_response(),
//     }
// }

// impl ManageIp{
//     pub fn get_ip(
//         state: &AppState,
//         desired_cidr: Option<i32>,
//     ) -> Result<IpAssignment, IpError> {
//         let cidr = desired_cidr.unwrap_or(24);
        
//         // Validate minimum slots requirement (at least 2 usable IPs)
//         let total_slots = Self::cidr_to_size(cidr)?;
//         if total_slots < 4 { // network, gateway, broadcast, + 1 device
//             return Err(IpError::InvalidArgs(
//                 format!("CIDR /{} too small, minimum /30 required (4 addresses)", cidr)
//             ));
//         }
        
//         let conn = get_conn(&state.db_pool)
//             .map_err(|e| IpError::SystemError(format!("DB connection failed: {}", e)))?;
        
//         // First, check recycle_ip_pool for exact CIDR match
//         if let Some(assignment) = Self::check_recycle_pool(&conn, cidr)? {
//             return Ok(assignment);
//         }
        
//         // If not found in recycle, search for larger subnets that can be split
//         if let Some(assignment) = Self::check_recycle_pool_larger(&conn, cidr)? {
//             return Ok(assignment);
//         }
        
//         // If nothing in recycle, allocate new from ip_pool
//         Self::allocate_new_range(&conn, cidr)
//     }

//     fn check_recycle_pool(
//         conn: &Connection,
//         cidr: i32,
//     ) -> Result<Option<IpAssignment>, IpError> {
//         let mut stmt = conn.prepare(
//             "SELECT ip, cidr FROM recycle_ip_pool WHERE cidr = ?1 ORDER BY ip LIMIT 1"
//         ).map_err(|e| IpError::SystemError(format!("Query failed: {}", e)))?;
        
//         let result = stmt.query_row(params![cidr], |row| {
//             Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i32>(1)?))
//         });
        
//         match result {
//             Ok((ip, recycle_cidr)) => {
//                 // Remove from recycle pool
//                 conn.execute(
//                     "DELETE FROM recycle_ip_pool WHERE ip = ?1 AND cidr = ?2",
//                     params![ip as i64, recycle_cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
                
//                 let broadcast = Self::calculate_broadcast(ip, cidr)?;
//                 let gateway = ip + 1;
//                 let total_slots = Self::cidr_to_size(cidr)?;
                
//                 Ok(Some(IpAssignment {
//                     ip,
//                     cidr,
//                     source: 2,
//                     gateway,
//                     broadcast,
//                     total_slots,
//                 }))
//             }
//               Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {
//                 Ok(None)
//             }
//             Err(e) => Err(IpError::SystemError(format!("Query error: {}", e))),
//         }
//     }

//     pub fn check_recycle_pool_larger(
//         conn: &Connection,
//         desired_cidr: i32,
//     ) -> Result<Option<IpAssignment>, IpError> {
//         // Look for larger subnets that can accommodate our needs
//         let mut stmt = conn.prepare(
//             "SELECT ip, cidr FROM recycle_ip_pool WHERE cidr < ?1 ORDER BY cidr ASC, ip LIMIT 1"
//         ).map_err(|e| IpError::SystemError(format!("Query failed: {}", e)))?;
        
//         let result = stmt.query_row(params![desired_cidr], |row| {
//             Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i32>(1)?))
//         });
        
//         match result {
//             Ok((ip, recycle_cidr)) => {
//                 let needed_size = Self::cidr_to_size(desired_cidr)?;
//                 let available_size = Self::cidr_to_size(recycle_cidr)?;
                
//                 // Calculate remaining portion
//                 let remaining_start = ip + needed_size;
//                 let remaining_size = available_size - needed_size;
                
//                 // Remove original from recycle
//                 conn.execute(
//                     "DELETE FROM recycle_ip_pool WHERE ip = ?1 AND cidr = ?2",
//                     params![ip as i64, recycle_cidr],
//                 ).map_err(|e| IpError::SystemError(format!("Delete recycle failed: {}", e)))?;
                
//                 // Insert remaining portion back if there's space left
//                 if remaining_size >= 4 { // Minimum /30
//                     let remaining_cidr = Self::size_to_cidr(remaining_size)?;
//                     conn.execute(
//                         "INSERT INTO recycle_ip_pool (ip, cidr) VALUES (?1, ?2)",
//                         params![remaining_start as i64, remaining_cidr],
//                     ).map_err(|e| IpError::SystemError(format!("Insert recycle failed: {}", e)))?;
//                 }
                
//                 let broadcast = Self::calculate_broadcast(ip, desired_cidr)?;
//                 let gateway = ip + 1;
//                 let total_slots = needed_size;
                
//                 Ok(Some(IpAssignment {
//                     ip,
//                     cidr: desired_cidr,
//                     source: 2,
//                     gateway,
//                     broadcast,
//                     total_slots,
//                 }))
//             }
//             Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {
//                 Ok(None)
//             }
//             Err(e) => Err(IpError::SystemError(format!("Query error: {}", e))),
//         }
//     }

//     fn allocate_new_range(
//         conn: &Connection,
//         cidr: i32,
//     ) -> Result<IpAssignment, IpError> {
//         let total_slots = Self::cidr_to_size(cidr)?;
//         let subnet_mask = Self::get_subnet_mask(cidr)?;
        
//         // Find the last allocated IP range
//         let mut stmt = conn.prepare(
//             "SELECT ip, cidr FROM ip_pool ORDER BY ip DESC LIMIT 1"
//         ).map_err(|e| IpError::SystemError(format!("Query failed: {}", e)))?;
        
//         let new_ip = match stmt.query_row([], |row| {
//             Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i32>(1)?))
//         }) {
//             Ok((last_ip, last_cidr)) => {
//                 let last_broadcast = Self::calculate_broadcast(last_ip, last_cidr)?;

//                 let next_ip = last_broadcast + 1;
                
//                 if next_ip & !subnet_mask != 0 {
//                     (next_ip + total_slots - 1) & subnet_mask
//                 } else {
//                     next_ip
//                 }
//             }
//             Err(sqlsrv::rusqlite::Error::QueryReturnedNoRows) => {
//                 Self::ip_to_int("172.16.0.0")?
//             }
//             Err(e) => return Err(IpError::SystemError(format!("Query error: {}", e))),
//         };
        
//         if new_ip > Self::ip_to_int("172.255.255.0")? {
//             return Err(IpError::AddressExhausted);
//         }
        
//         let broadcast = Self::calculate_broadcast(new_ip, cidr)?;
//         let gateway = new_ip + 1;
        
//         Ok(IpAssignment {
//             ip: new_ip,
//             cidr,
//             source: 1,
//             gateway,
//             broadcast,
//             total_slots,
//         })
//     }
// }