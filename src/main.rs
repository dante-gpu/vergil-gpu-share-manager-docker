use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

mod core;
mod gpu;
mod monitoring;
mod api;
mod utils;
mod config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting GPU Share VM Manager");

    // Initialize core components
    let libvirt = Arc::new(Mutex::new(core::LibvirtManager::new()?));
    let gpu_manager = Arc::new(Mutex::new(gpu::GPUManager::new()?));
    let metrics = Arc::new(Mutex::new(monitoring::MetricsCollector::new(
        5, // 5 second collection interval
        24, // 24 hour retention
    )));

    // Shutdown mechanism for graceful shutdown
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();

    // Initialize application state
    let state = Arc::new(api::AppState {
        libvirt,
        gpu_manager,
        metrics,
        shutdown_signal: Arc::new(Mutex::new(shutdown_sender)),
        shutdown_receiver: Arc::new(Mutex::new(shutdown_receiver)),
    });

    // Create API router
    let app = api::create_router(state);

    // Start the server
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Server listening on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}