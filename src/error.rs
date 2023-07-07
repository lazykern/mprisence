#[derive(Debug)]
pub enum Error {
    UpdateError(String),
    ProviderError(String),
    DiscordError(Box<dyn std::error::Error>),
    RenderError(handlebars::RenderError),
    TemplateError(handlebars::TemplateError),
    LoftyError(lofty::LoftyError),
    DBusError(mpris::DBusError),
    ImgBBError(imgbb::Error),
    ReqwestError(reqwest::Error),
}

impl From<handlebars::RenderError> for Error {
    fn from(error: handlebars::RenderError) -> Self {
        Error::RenderError(error)
    }
}

impl From<handlebars::TemplateError> for Error {
    fn from(error: handlebars::TemplateError) -> Self {
        Error::TemplateError(error)
    }
}

impl From<lofty::LoftyError> for Error {
    fn from(error: lofty::LoftyError) -> Self {
        Error::LoftyError(error)
    }
}

impl From<mpris::DBusError> for Error {
    fn from(error: mpris::DBusError) -> Self {
        Error::DBusError(error)
    }
}

impl From<imgbb::Error> for Error {
    fn from(error: imgbb::Error) -> Self {
        Error::ImgBBError(error)
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::ReqwestError(error)
    }
}
