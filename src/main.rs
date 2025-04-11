pub mod config;

fn main() {
    let config = config::fetch_config().unwrap();
    println!("config {:?}", config);
}
