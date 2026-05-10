use axum::{Router, routing::post};

use crate::my_states::AppState;

pub fn get_bridge_routers()->Router<AppState>
{
    Router::new()
    // .route("/get_ip_range", post(super::get_ip_range2::handle_get_ip))
}