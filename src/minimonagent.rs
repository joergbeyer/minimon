use axum::{routing::get, Router};
use clap::Parser;
use dotenv::dotenv;
use minimonitor::{home, measure_disk_thread, AppState, DiskMeasurement};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::{Arc, Mutex};
use sysinfo::System;
use tokio::net::TcpListener;

fn get_my_version() -> String {
    let version = match env::var("VERSION") {
        Ok(val) => val,
        Err(_e) => "unkown_version".to_string(),
    };
    let release = match env::var("RELEASE") {
        Ok(val) => val,
        Err(_e) => "unkown_release".to_string(),
    };

    format!("{version}-{release}")
}

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
    dotenv().ok();

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
        measurements: Arc::new(Mutex::new(
            HashMap::<String, VecDeque<DiskMeasurement>>::new(),
        )),
        hostname,
        versionstr,
    };

    measure_disk_thread(shared_state.measurements.clone());
    let app = Router::new().route("/", get(home)).with_state(shared_state);

    let listener = TcpListener::bind("0.0.0.0:".to_string() + &args.port.to_string())
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
