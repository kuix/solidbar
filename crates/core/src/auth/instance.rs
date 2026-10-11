use solidbar_shared::instance::InstanceSettings;
use thiserror::Error;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum InstanceError {
    #[error("Enter the address of your solidtime instance, e.g. https://solidtime.example.com")]
    InvalidUrl,
    #[error("Use https:// (plain http is only allowed for localhost)")]
    InsecureUrl,
    #[error("Enter the OAuth client ID")]
    MissingClientId,
}

/// A validated solidtime instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    /// Always ends with `/` so relative API paths join below it, even with a path prefix.
    base: Url,
    client_id: String,
}

impl Instance {
    pub fn new(settings: &InstanceSettings) -> Result<Self, InstanceError> {
        let mut base = Url::parse(settings.url.trim()).map_err(|_| InstanceError::InvalidUrl)?;
        let local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        match base.scheme() {
            "https" => {}
            "http" if local => {}
            "http" => return Err(InstanceError::InsecureUrl),
            _ => return Err(InstanceError::InvalidUrl),
        }
        if base.host_str().is_none() {
            return Err(InstanceError::InvalidUrl);
        }
        base.set_query(None);
        base.set_fragment(None);
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }

        let client_id = settings.client_id.trim().to_owned();
        if client_id.is_empty() {
            return Err(InstanceError::MissingClientId);
        }
        Ok(Self { base, client_id })
    }

    /// `path` is relative, e.g. `api/v1/users/me`.
    pub fn endpoint(&self, path: &str) -> Url {
        self.base
            .join(path)
            .expect("relative API paths are valid URLs")
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Identifies the stored session: one per instance and OAuth client.
    pub fn account(&self) -> String {
        format!("{}|{}", self.display_url(), self.client_id)
    }

    /// The instance address as people typed it, without the trailing slash.
    pub fn display_url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_owned()
    }

    /// The address of a page in the solidtime web app, e.g. `time` for the time tracker.
    pub fn web_url(&self, path: &str) -> Url {
        self.endpoint(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(url: &str) -> InstanceSettings {
        InstanceSettings {
            url: url.to_owned(),
            ..InstanceSettings::cloud()
        }
    }

    #[test]
    fn joins_api_paths_below_the_base_including_a_path_prefix() {
        let cloud = Instance::new(&InstanceSettings::cloud()).unwrap();
        assert_eq!(
            cloud.endpoint("api/v1/users/me").as_str(),
            "https://app.solidtime.io/api/v1/users/me"
        );
        let prefixed = Instance::new(&settings(" https://example.com/solidtime/?x=1 ")).unwrap();
        assert_eq!(
            prefixed.endpoint("oauth/token").as_str(),
            "https://example.com/solidtime/oauth/token"
        );
        assert_eq!(prefixed.display_url(), "https://example.com/solidtime");
    }

    #[test]
    fn requires_https_except_on_localhost() {
        assert_eq!(
            Instance::new(&settings("http://solidtime.example.com")),
            Err(InstanceError::InsecureUrl)
        );
        assert!(Instance::new(&settings("http://localhost:8000")).is_ok());
        assert_eq!(
            Instance::new(&settings("solidtime.example.com")),
            Err(InstanceError::InvalidUrl)
        );
        assert_eq!(
            Instance::new(&settings("ftp://example.com")),
            Err(InstanceError::InvalidUrl)
        );
    }

    #[test]
    fn requires_a_client_id() {
        let mut missing = InstanceSettings::cloud();
        missing.client_id = "  ".to_owned();
        assert_eq!(Instance::new(&missing), Err(InstanceError::MissingClientId));
    }

    #[test]
    fn account_identifies_instance_and_client() {
        let cloud = Instance::new(&InstanceSettings::cloud()).unwrap();
        assert_eq!(
            cloud.account(),
            "https://app.solidtime.io|9c994748-c593-4a6d-951b-6849c829bc4e"
        );
    }
}
