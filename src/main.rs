use mprisence::Mprisence;
use tokio;

#[tokio::main]
async fn main() {
    Mprisence::new().start().await;
}
