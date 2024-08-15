// build.rs

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("version.rs");
    fs::write(
        &dest_path,
        "pub fn get_my_version() -> String {
            let version = match env::var(\"VERSION\") {
                Ok(val) => val,
                Err(_e) => \"unkown_version\".to_string(),
            };
            let release = match env::var(\"RELEASE\") {
                Ok(val) => val,
                Err(_e) => \"unkown_release\".to_string(),
            };

            format!(\"{version}-{release}\")
        }
        ",
    )
    .unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}
