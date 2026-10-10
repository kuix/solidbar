use std::time::Duration;

use thiserror::Error;

use crate::auth::AuthError;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ApiError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("Can't reach solidtime. Check your connection.")]
    Offline,
    #[error("solidtime is busy. Try again in a moment.")]
    RateLimited { retry_after: Option<Duration> },
    /// A business rule rejected the request (solidtime's `ApiException`), e.g. an overlap.
    #[error("{message}")]
    Rejected { key: String, message: String },
    /// Laravel validation failed (HTTP 422).
    #[error("{0}")]
    Invalid(String),
    #[error("You don't have permission to do that in this organization")]
    Forbidden,
    #[error("That item no longer exists in solidtime")]
    NotFound,
    #[error("solidtime returned an unexpected response ({0})")]
    Unexpected(String),
}

impl ApiError {
    /// The organization rejects overlapping time entries.
    pub fn is_overlap(&self) -> bool {
        matches!(self, Self::Rejected { key, .. } if key == "overlapping_time_entry")
    }

    /// Another timer is already running.
    pub fn is_timer_running(&self) -> bool {
        matches!(self, Self::Rejected { key, .. } if key == "time_entry_still_running")
    }

    pub fn is_offline(&self) -> bool {
        matches!(self, Self::Offline | Self::Auth(AuthError::Network(_)))
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_connect() || error.is_timeout() || error.is_request() {
            Self::Offline
        } else {
            Self::Unexpected(error.to_string())
        }
    }
}
