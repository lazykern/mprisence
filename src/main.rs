use mprisence::Mprisence;
use tokio;

#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("Starting mprisence");
    Mprisence::new().start().await;
}
