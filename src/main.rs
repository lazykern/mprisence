mod config;

fn main() {
    let config = match config::load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    };

    println!("{:?}", config);
}
