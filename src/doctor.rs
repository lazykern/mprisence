use crate::config;
use crate::discord;
use crate::error::Error;
use crate::web_bridge;

pub async fn run(fix: bool) -> Result<(), Error> {
    println!("mprisence doctor\n");

    let mut issues = 0;

    let config_path = config::config_path()?;
    if config_path.exists() {
        match config::validate_config_file(Some(&config_path)) {
            Ok(_) => println!("✓ Config: valid ({})", config_path.display()),
            Err(err) => {
                issues += 1;
                println!("✗ Config: invalid ({}) — {err}", config_path.display());
            }
        }
    } else {
        match config::validate_config_file(None) {
            Ok(_) => {
                println!(
                    "○ Config: no user file (bundled defaults OK, path: {})",
                    config_path.display()
                );
            }
            Err(err) => {
                issues += 1;
                println!("✗ Config: bundled defaults failed to load — {err}");
            }
        }
    }

    if discord::is_discord_running() {
        println!("✓ Discord: IPC socket found");
    } else {
        issues += 1;
        println!("✗ Discord: not running or IPC socket not found");
    }

    if web_bridge::check_dbus_session() {
        println!("✓ D-Bus: session bus available");
    } else {
        issues += 1;
        println!("✗ D-Bus: DBUS_SESSION_BUS_ADDRESS not set");
    }

    println!();
    println!("Web bridge");
    issues += web_bridge::check_native_host_manifests();

    if fix && issues > 0 {
        println!();
        println!("Attempting fixes...");
        web_bridge::install(Vec::new()).await;
        println!("Re-run `mprisence doctor` to verify.");
    }

    println!();
    if issues > 0 {
        if fix {
            eprintln!("{issues} issue(s) remain after fix attempt.");
        } else {
            eprintln!("{issues} issue(s) found. Run `mprisence doctor --fix` to install web bridge manifests.");
        }
        std::process::exit(1);
    }

    println!("All checks passed.");
    Ok(())
}
