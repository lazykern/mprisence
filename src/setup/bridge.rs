use std::path::PathBuf;

use crate::error::Error;
use crate::setup::ui::{self, confirm_save, prompt_select, BACK_LABEL};
use crate::web_bridge;

const EXTENSION_MANIFEST: &str = "extension/dist/firefox/manifest.json";

pub fn run() -> Result<(), Error> {
    loop {
        ui::redraw(&["Settings", "Web bridge"], "Web bridge")?;

        let issues = web_bridge::native_host_issue_count();
        if issues == 0 {
            ui::print_dim("Native host manifests: ok");
        } else {
            ui::print_dim(&format!("Native host manifests: {issues} issue(s)"));
        }

        if web_bridge::check_dbus_session() {
            ui::print_dim("D-Bus session: available");
        } else {
            ui::print_dim("D-Bus session: DBUS_SESSION_BUS_ADDRESS not set");
        }

        ui::print_dim(&extension_status_line());
        println!();

        let choice = match prompt_select(
            "",
            vec![
                "Install native host manifests".to_string(),
                "Run web bridge doctor".to_string(),
                BACK_LABEL.to_string(),
            ],
        )? {
            Some(value) => value,
            None => return Ok(()),
        };

        match choice.as_str() {
            "Install native host manifests" => run_install()?,
            "Run web bridge doctor" => run_doctor()?,
            BACK_LABEL => return Ok(()),
            _ => {}
        }
    }
}

fn extension_status_line() -> String {
    let path = extension_manifest_path();
    if path.exists() {
        format!("Extension build: found ({})", path.display())
    } else {
        "Extension build: not found (run: cd extension && node build.mjs firefox)".to_string()
    }
}

fn extension_manifest_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(EXTENSION_MANIFEST)
}

fn run_install() -> Result<(), Error> {
    if !confirm_save("Install native host manifests for Firefox and Chromium?")? {
        return Ok(());
    }
    ui::redraw(&["Settings", "Web bridge", "Install"], "Installing")?;
    ui::print_dim("Installing manifests…");
    block_on_async(web_bridge::install(Vec::new()));
    ui::print_dim("Restart browser if needed.");
    println!();
    for line in web_bridge::doctor_lines() {
        if !line.is_empty() {
            ui::print_dim(&line);
        }
    }
    let _ = prompt_select("", vec![BACK_LABEL.to_string()])?;
    Ok(())
}

fn run_doctor() -> Result<(), Error> {
    ui::redraw(&["Settings", "Web bridge", "Doctor"], "Web bridge doctor")?;
    for line in web_bridge::doctor_lines() {
        if line.is_empty() {
            println!();
        } else {
            ui::print_dim(&line);
        }
    }
    println!();
    let _ = prompt_select("", vec![BACK_LABEL.to_string()])?;
    Ok(())
}

fn block_on_async<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}
