//! The state of the in-app updater, shown in Settings and, once an update is found, as a
//! banner above the day list.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateState {
    /// Not checked yet (or only background checks ran, and they failed quietly).
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    /// Downloading and installing; `percent` is `None` while the size is unknown.
    Installing {
        version: String,
        percent: Option<u8>,
    },
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateView {
    pub current_version: String,
    pub state: UpdateState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_tagged() {
        let json = serde_json::to_value(UpdateState::Installing {
            version: "0.2.0".into(),
            percent: Some(40),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "state": "installing", "version": "0.2.0", "percent": 40 })
        );
    }
}
