use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("You're not signed in")]
    SignedOut,
    #[error("Your solidtime session has expired. Please sign in again.")]
    SessionExpired,
    #[error("Sign-in was cancelled")]
    Cancelled,
    #[error("Sign-in timed out. Please try again.")]
    TimedOut,
    #[error("The sign-in response didn't belong to this request. Please try again.")]
    StateMismatch,
    #[error("solidtime refused the sign-in: {0}")]
    Denied(String),
    #[error("Port {0} is already in use. Choose another port in the instance settings.")]
    PortInUse(u16),
    #[error("Can't reach solidtime: {0}")]
    Network(String),
    #[error("Unexpected response from solidtime: {0}")]
    Protocol(String),
    #[error("Couldn't access the saved session: {0}")]
    Store(String),
}

impl From<reqwest::Error> for AuthError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_decode() {
            Self::Protocol(error.to_string())
        } else {
            Self::Network(error.to_string())
        }
    }
}
