mod config;

fn main() {
    let config = config::fetch_config().unwrap();
    dbg!(&config);
}
