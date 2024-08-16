use axum::{routing::get, Router};
use clap::Parser;
use minimonitor::{
    collect_remote_disk_thread, current, home, measure_local_disk_thread, overview, AppState,
    DiskMeasurement, DiskMeasurementMap,
};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::{Arc, Mutex};
use sysinfo::System;
use tokio::net::TcpListener;

// auto generated with build.rs
include!(concat!(env!("OUT_DIR"), "/version.rs"));

#[derive(Parser, Debug)]
#[command(version, about, long_about = None, disable_version_flag = true)]
struct Args {
    #[arg(short, long, default_value_t = 60)]
    interval: u32,

    #[arg(short, long, default_value_t = 9988)]
    port: u32,

    #[clap(short, long)]
    version: bool,
}

#[tokio::main]
async fn main() {
    let versionstr = get_my_version();
    let args = Args::parse();

    if args.version {
        println!("version: {}", versionstr);
        return;
    }

    let hostname = match System::host_name() {
        Some(name) => name,
        None => "horse_with_no_name".to_string(),
    };

    let shared_state = AppState {
        local_measurements: Arc::new(Mutex::new(DiskMeasurementMap::new())),
        remote_measurements: Arc::new(Mutex::new(
            HashMap::<(String, String), DiskMeasurement>::new(),
        )),
        hostname,
        versionstr,
    };

    measure_local_disk_thread(shared_state.local_measurements.clone(), args.interval);
    collect_remote_disk_thread(shared_state.remote_measurements.clone(), args.interval);
    let app = Router::new()
        .route("/", get(home))
        .route("/current", get(current))
        .route("/overview", get(overview))
        .with_state(shared_state);

    let listener = TcpListener::bind("0.0.0.0:".to_string() + &args.port.to_string())
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
