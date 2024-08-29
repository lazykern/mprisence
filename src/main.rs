use clap::{Args, Parser, Subcommand};
use mprisence::Mprisence;
use tokio;

#[derive(Debug, Parser)]
#[command(name = "mprisence")]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(name = "start", about = "Start the mprisence daemon")]
    Start,
    #[command(name = "player", about = "Commands for interacting with players")]
    Player(PlayerArgs),
}

#[derive(Debug, Args)]
struct PlayerArgs {
    #[command(subcommand)]
    command: PlayerCommands,
}

#[derive(Debug, Subcommand)]
enum PlayerCommands {
    #[command(name = "list", about = "List all available players")]
    List,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Start) | None => {
            env_logger::init();
            log::info!("Starting mprisence");
            Mprisence::new().start().await;
        }
        Some(Commands::Player(player)) => match player.command {
            PlayerCommands::List => {
                let players = mprisence::player::get_players();

                println!("Found {} players", players.len());

                for player in players {
                    print!("- {}", player.identity());

                    if let Ok(metadata) = player.get_metadata() {
                        if let Some(title) = metadata.url() {
                            print!(" : {}", title);
                        }
                    }

                    println!();
                }
            }
        },
    }
}
