use axum::{Router, routing::post};

use crate::my_states::AppState;

pub fn get_vm_routers()->Router<AppState>
{
    Router::new()
    .route("/start_vm", post(super::start_vm::handle_start_vm))
}