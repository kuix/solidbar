//! Seams to the outside world that tests replace.

use jiff::Timestamp;
use solidbar_shared::view::{AppView, Catalog, Notice};

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Delivers view updates to the UI (and the menu bar).
pub trait EventSink: Send + Sync {
    fn app_changed(&self, view: &AppView);
    fn catalog_changed(&self, catalog: &Catalog);
    fn notice(&self, notice: &Notice);
}

/// Opens a URL in the default browser.
pub trait UrlOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;

    use jiff::{SignedDuration, Timestamp};

    use super::Clock;

    pub struct FixedClock(Mutex<Timestamp>);

    impl FixedClock {
        pub fn at(text: &str) -> Self {
            Self(Mutex::new(text.parse().unwrap()))
        }

        pub fn advance(&self, by: SignedDuration) {
            let mut now = self.0.lock().unwrap();
            *now += by;
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            *self.0.lock().unwrap()
        }
    }
}
