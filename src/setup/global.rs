use std::path::Path;

use crate::error::Error;
use crate::setup::patch::{apply_edit, ConfigEdit};
use crate::setup::ui;

pub fn save_global_edit(config_path: &Path, edit: &ConfigEdit, confirm: &str) -> Result<bool, Error> {
    if edit.is_empty() {
        return Ok(false);
    }
    if !ui::confirm_save(confirm)? {
        return Ok(false);
    }
    apply_edit(config_path, edit)?;
    ui::print_dim(&format!("Saved to {}", config_path.display()));
    Ok(true)
}
