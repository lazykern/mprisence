use std::io::{self, IsTerminal, Write};

use crossterm::{
    cursor,
    execute,
    style::{Attribute, Print, ResetColor, SetAttribute},
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use inquire::{Confirm, InquireError, Select, Text};
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};

use crate::error::Error;

pub const BACK_LABEL: &str = "← Back";

/// Keeps setup inside the alternate screen; restores on drop.
pub struct SetupTerminal;

impl SetupTerminal {
    pub fn enter() -> Result<Self, Error> {
        require_tty()?;
        configure_inquire_theme();
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        Ok(Self)
    }
}

impl Drop for SetupTerminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn require_tty() -> Result<(), Error> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return Ok(());
    }
    Err(Error::IO(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "setup requires an interactive terminal; use `mprisence config edit` instead",
    )))
}

/// Clear alternate screen and print breadcrumb + title (static content only).
pub fn redraw(path: &[&str], title: &str) -> Result<(), Error> {
    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    print_header(path, title);
    Ok(())
}

fn print_header(path: &[&str], title: &str) {
    let mut stdout = io::stdout();
    let breadcrumb = path.join(" › ");
    let _ = execute!(
        stdout,
        SetAttribute(Attribute::Dim),
        Print(format!("{breadcrumb}\n")),
        ResetColor
    );
    if !title.is_empty() {
        let _ = execute!(
            stdout,
            SetAttribute(Attribute::Bold),
            Print(format!("{title}\n")),
            ResetColor
        );
    }
    let _ = stdout.flush();
}

pub fn print_dim(line: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        SetAttribute(Attribute::Dim),
        Print(format!("{line}\n")),
        ResetColor
    );
}

pub fn prompt_select<T: std::fmt::Display>(message: &str, options: Vec<T>) -> Result<Option<T>, Error> {
    match Select::new(message, options).prompt_skippable() {
        Ok(choice) => Ok(choice),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(err) => Err(prompt_err(err)),
    }
}

pub fn prompt_text(message: &str, default: &str, help: &str) -> Result<String, Error> {
    let mut prompt = Text::new(message).with_default(default);
    if !help.is_empty() {
        prompt = prompt.with_help_message(help);
    }
    match prompt.prompt() {
        Ok(value) => Ok(value),
        Err(InquireError::OperationCanceled) => Err(canceled()),
        Err(err) => Err(prompt_err(err)),
    }
}

pub fn confirm_save(message: &str) -> Result<bool, Error> {
    match Confirm::new(message).with_default(true).prompt() {
        Ok(value) => Ok(value),
        Err(InquireError::OperationCanceled) => Ok(false),
        Err(err) => Err(prompt_err(err)),
    }
}

fn configure_inquire_theme() {
    let base = RenderConfig::default()
        .with_prompt_prefix(Styled::new("›").with_fg(Color::LightGreen))
        .with_answered_prompt_prefix(Styled::new("›").with_fg(Color::DarkGreen))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_canceled_prompt_indicator(Styled::new("<canceled>").with_fg(Color::DarkGrey))
        .with_selected_option(Some(
            StyleSheet::new()
                .with_fg(Color::LightYellow)
                .with_attr(Attributes::BOLD),
        ))
        .with_option(StyleSheet::new().with_fg(Color::Grey));

    inquire::set_global_render_config(base);
}

fn canceled() -> Error {
    Error::IO(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "setup canceled",
    ))
}

fn prompt_err(err: InquireError) -> Error {
    Error::IO(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    ))
}
