use ff_sync::ApplyError;

/// Everything the Kotlin side can be handed back as a thrown exception.
///
/// Deliberately few variants, and each one maps to something the Android
/// UI says differently (DESIGN.md §11: *data freshness is explicit, never
/// silent* — a pilot must never read "no results" when the truth is "no
/// cycle downloaded"). [`CoreError::NoCycle`] in particular exists so an
/// empty map is impossible to confuse with an unsynced one.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("no cycle bundle has been downloaded yet")]
    NoCycle,
    #[error("{0} not found in the current cycle")]
    NotFound(String),
    #[error("cycle database error: {0}")]
    Database(String),
    #[error("chart tile error: {0}")]
    Chart(String),
    #[error("{0}")]
    Sync(String),
    #[error("invalid cycle manifest: {0}")]
    InvalidManifest(String),
    #[error("invalid points JSON: {0}")]
    InvalidPoints(String),
    #[error("invalid profile JSON: {0}")]
    InvalidProfile(String),
    #[error("invalid winds JSON: {0}")]
    InvalidWinds(String),
    #[error("invalid track JSON: {0}")]
    InvalidTrack(String),
    #[error("failed to serialize result: {0}")]
    Serialize(String),
}

impl From<rusqlite::Error> for CoreError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => CoreError::NotFound("row".to_string()),
            other => CoreError::Database(other.to_string()),
        }
    }
}

impl From<ApplyError> for CoreError {
    fn from(err: ApplyError) -> Self {
        CoreError::Sync(err.to_string())
    }
}
