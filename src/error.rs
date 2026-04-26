use thiserror::Error;

#[derive(Debug, Error)]
pub enum KbError {
    #[error("configuration file not found; run `obsidian-kb init <vault>` or pass --vault")]
    MissingConfig,

    #[error("search query is empty")]
    EmptyQuery,

    #[error("index not found; run `obsidian-kb index --vault <vault>` first")]
    MissingIndex,
}
