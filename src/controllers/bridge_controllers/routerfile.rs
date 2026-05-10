use axum::{Router, routing::{delete, post}};

use crate::my_states::AppState;

pub fn get_bridge_routers()->Router<AppState>
{
    Router::new()
    .route("/create_bridge", post(super::create_bridge::handle_create_bridge))
    .route("/delete_bridge/:bridge_name", delete(super::delete_bridge::handle_delete_bridge))
}