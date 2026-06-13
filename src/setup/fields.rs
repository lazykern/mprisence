use inquire::{Confirm, InquireError, Text};

use crate::error::Error;

pub fn prompt_text_with_help(
    message: &str,
    default: &str,
    help: &str,
) -> Result<Option<String>, Error> {
    let mut prompt = Text::new(message).with_default(default);
    if !help.is_empty() {
        prompt = prompt.with_help_message(help);
    }
    match prompt.prompt() {
        Ok(value) => Ok(Some(value)),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(err) => Err(prompt_err(err)),
    }
}

pub fn prompt_optional_text_with_help(
    message: &str,
    default: Option<&str>,
    help: &str,
) -> Result<Option<String>, Error> {
    let default_str = default.unwrap_or("");
    let value = match prompt_text_with_help(message, default_str, help)? {
        Some(value) => value,
        None => return Ok(None),
    };
    if value.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub fn prompt_bool_with_help(
    message: &str,
    default: bool,
    help: &str,
) -> Result<Option<bool>, Error> {
    let mut prompt = Confirm::new(message).with_default(default);
    if !help.is_empty() {
        prompt = prompt.with_help_message(help);
    }
    match prompt.prompt() {
        Ok(value) => Ok(Some(value)),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(err) => Err(prompt_err(err)),
    }
}

fn prompt_err(err: InquireError) -> Error {
    Error::IO(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    ))
}
