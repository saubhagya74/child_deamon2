use axum::{Router, routing::{delete, post}};

use crate::my_states::AppState;

pub fn get_tap_routers()->Router<AppState>
{
    Router::new()
    .route("/create_tap", post(super::create_tap::handle_create_tap))
    .route("/delete_tap/:tap_name", delete(super::delete_tap::handle_delete_tap))
}