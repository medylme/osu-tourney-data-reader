use std::env;
use winresource::WindowsResource;

fn main() {
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let _ = WindowsResource::new().set_icon("assets/icon.ico").compile();
    }

    println!("cargo:rerun-if-changed=.env");
}
