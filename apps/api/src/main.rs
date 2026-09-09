mod analytics;
mod auth;
mod auth_middleware;
mod config;
mod device_registry;
mod handlers;
mod models;
mod routes;
mod services;
mod stellar_service;
mod webhook;
mod webhook_handlers;
mod webhook_service;

use axum::{http::Method, Router};
use std::env;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use webhook_service::WebhookStore;

fn bind_addr() -> SocketAddr {
    let port = env::var("PORT")
        .ok()
        .map(|value| value.parse::<u16>().unwrap_or(8000))
        .unwrap_or(8000);

    SocketAddr::from(([0, 0, 0, 0], port))
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let store = WebhookStore::new();

    let app = Router::new()
        .merge(routes::auth_routes())
        .merge(routes::device_routes())
        .merge(routes::payment_routes())
        .merge(routes::earnings_routes())
        .merge(routes::webhook_routes().with_state(store))
        .layer(cors);

    // Start background task for heartbeat checking
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            services::check_offline_devices();
        }
    });

    // Start server
    let addr = bind_addr();
    println!("🚀 Server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::bind_addr;
    use std::env;

    #[test]
    fn bind_addr_honors_port_environment_variable() {
        let previous = env::var("PORT").ok();
        env::set_var("PORT", "8123");

        let addr = bind_addr();
        assert_eq!(addr.port(), 8123);

        match previous {
            Some(value) => env::set_var("PORT", value),
            None => env::remove_var("PORT"),
        }
    }
}