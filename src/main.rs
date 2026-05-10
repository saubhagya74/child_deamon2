use axum::{
    Router, body::Body, middleware, response::{IntoResponse, Response}, routing::post
};
use hyper_util::rt::TokioIo;
use std::{fs, path::Path, sync::Arc};
use tokio::{
    net::UnixListener,
    process::Command,
};
use tokio;
use async_stream;
use tower::Service;
use tracing::{error, info};
use tokio::time::Duration;


mod controllers;
mod my_states;
mod db_services;

const SOCKET_PATH: &str = "/tmp/htop-server.sock";


#[tokio::main]
async fn main() {
    
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "unix_htop_server=info".into()),
        )
        .init();
    // Remove stale socket file
    if Path::new(SOCKET_PATH).exists() {
        fs::remove_file(SOCKET_PATH).expect("Failed to remove old socket");
    }

    let shared_state=my_states::initialize_state().await;
    
    let app = Router::new()
        .route("/health", post(health))
        .route("/stream_storage", post(stream_storage))
        .route("/check_storage", post(check_storage))
        // ;
        .merge(controllers::tap_controllers::routerfile::get_tap_routers())
        .merge(controllers::bridge_controllers::routerfile::get_bridge_routers())
        .merge(controllers::vm_controllers::routerfile::get_vm_routers())
        // .layer(middleware::from_fn_with_state(        // ← add this
        //     shared_state.clone(),
        //     log_state_middleware,
        // ))
        .with_state(shared_state);
    

    let listener = UnixListener::bind(SOCKET_PATH).expect("Failed to bind Unix socket");

    info!("🚀 Unix socket server listening on {SOCKET_PATH}");
    info!("   Stream htop: curl -N --unix-socket {SOCKET_PATH} http://localhost/stream");
    info!("   Health:       curl --unix-socket {SOCKET_PATH} http://localhost/health");

    // Axum doesn't expose a Unix-socket `serve` directly for hyper 1.x,
    // so we accept connections ourselves and hand them to a hyper service.
    let app = Arc::new(app);

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Accept error: {e}");
                continue;
            }
        };

        let app = Arc::clone(&app);

        tokio::spawn(async move {
            let io = TokioIo::new(stream);

            // Build a hyper service from the axum router
            let hyper_service = hyper::service::service_fn(move |req| {
                let app = Arc::clone(&app);
                async move { app.as_ref().clone().call(req).await }
            });

            if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                hyper_util::rt::TokioExecutor::new(),
            )
            .serve_connection(io, hyper_service)
            .await
            {
                if !e.to_string().contains("connection reset") {
                    error!("Connection error: {e}");
                }
            }
        });
    }
}

async fn health() -> &'static str {
    "OK\n"
}

async fn check_storage() -> impl IntoResponse {
    let output = Command::new("df")
        .arg("-h")
        .output()
        .await
        .expect("failed to run df -h");

    String::from_utf8_lossy(&output.stdout).to_string()
}

async fn stream_storage() -> impl IntoResponse {
    let storage_stream = async_stream::stream! {
        loop {
            let output = Command::new("df")
                .arg("-h")
                .output()
                .await
                .unwrap();
            
            yield Ok::<_, std::io::Error>(format!("\x1b[2J\x1b[H{}", String::from_utf8_lossy(&output.stdout)));

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Response::builder()
        .header("Content-Type", "text/plain")
        .body(Body::from_stream(storage_stream))
        .unwrap()
}
