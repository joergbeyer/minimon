use axum::{
    routing::get,
    Router,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use sysinfo::System;
use minimonitor::{DiskMeasurement, AppState, measure_disk_thread, home};

#[tokio::main]
async fn main() {
    let hostname = match System::host_name() {
        Some(name) => name,
        None => "horse_with_no_name".to_string(),
    };

    let shared_state = AppState {
        measurements: Arc::new(Mutex::new(
            HashMap::<String, VecDeque<DiskMeasurement>>::new(),
        )),
        hostname,
    };

    measure_disk_thread(shared_state.measurements.clone());
    let app = Router::new().route("/", get(home)).with_state(shared_state);

    let listener = TcpListener::bind("0.0.0.0:9988").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
