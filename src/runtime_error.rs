#[derive(thiserror::Error, Debug)]
pub enum JsonError {
    #[error("JSON value isn't string")]
    AsStr,
    #[error("JSON value isn't array")]
    AsArray,
}
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Internal error: {0}")]
    Json(#[from] JsonError),
    #[error(transparent)]
    Slack(#[from] crate::slack::SlackError),
    #[error(transparent)]
    Linux(#[from] crate::linux_user_manage::LinuxError),
    #[error("Internal error: path -> str conversion failed")]
    PathToStr,
    #[error("there is no channel named {0}")]
    NoChannel(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("chrono parse error: {0}")]
    ChronoParse(#[from] chrono::ParseError),
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("goodbye event was caught. try to reconnect...")]
    CaughtGoodBye,
    #[error("receive non-event object on RTM")]
    NonEvent,
}
pub type Result<T> = std::result::Result<T, Error>;
/// JSON値を文字列として取得
pub fn as_str(v: &serde_json::Value) -> Result<&str> {
    Ok(v.as_str().ok_or(JsonError::AsStr)?)
}

/// JSON値を配列として取得
pub fn as_array(v: &serde_json::Value) -> Result<&Vec<serde_json::Value>> {
    Ok(v.as_array().ok_or(JsonError::AsArray)?)
}

pub fn path_join(v: &[&str]) -> Result<String> {
    Ok(v.iter()
        .collect::<std::path::PathBuf>()
        .to_str()
        .ok_or_else(|| Error::PathToStr)?
        .to_string())
}
