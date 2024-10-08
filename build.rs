// build.rs

use dotenv;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    dotenv::dotenv().ok();

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("version.rs");
    let version = match env::var("VERSION") {
        Ok(val) => val,
        Err(_e) => "unkown_version".to_string(),
    };
    let release = match env::var("RELEASE") {
        Ok(val) => val,
        Err(_e) => "unkown_release".to_string(),
    };
    let version_str = format!("{version}-{release}");

    let my_fun = format!(
        "pub fn get_my_version() -> String {{ \"{}\".to_string() }}",
        version_str
    );

    fs::write(&dest_path, my_fun).unwrap();
    println!("cargo::rerun-if-changed=.env");
    //println!("cargo::rerun-if-changed=build.rs");
}
