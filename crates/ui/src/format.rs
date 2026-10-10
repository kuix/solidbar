//! Text for durations and entries, on screen and for VoiceOver.

use solidbar_shared::update::{UpdateState, UpdateView};
use solidbar_shared::view::EntryView;

pub use solidbar_shared::duration::format_hm;

/// `5400` → `1 hour 30 minutes`, for screen readers (they read `1:30` as a clock time).
pub fn spoken_duration(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    let unit = |count: i64, singular: &str| match count {
        1 => format!("1 {singular}"),
        count => format!("{count} {singular}s"),
    };
    match (hours, minutes) {
        (0, minutes) => unit(minutes, "minute"),
        (hours, 0) => unit(hours, "hour"),
        (hours, minutes) => format!("{} {}", unit(hours, "hour"), unit(minutes, "minute")),
    }
}

/// `Design system › Header`, or `No project`.
pub fn work_title(entry: &EntryView) -> String {
    match (&entry.project_name, &entry.task_name) {
        (Some(project), Some(task)) => format!("{project} › {task}"),
        (Some(project), None) => project.clone(),
        (None, _) if entry.project_id.is_some() => "Unknown project".to_owned(),
        (None, _) => "No project".to_owned(),
    }
}

/// Everything a screen reader needs about an entry in one sentence.
pub fn entry_label(entry: &EntryView) -> String {
    let mut parts = vec![work_title(entry)];
    if let Some(client) = &entry.client_name {
        parts.push(client.clone());
    }
    if !entry.description.is_empty() {
        parts.push(entry.description.clone());
    }
    let until = entry.end_label.as_deref().unwrap_or("now");
    parts.push(format!(
        "{}, {} to {until}",
        spoken_duration(entry.duration_seconds),
        entry.start_label
    ));
    if entry.running {
        parts.push("timer running".to_owned());
    }
    if entry.billable {
        parts.push("billable".to_owned());
    }
    if entry.pending {
        parts.push("saving".to_owned());
    }
    parts.join(". ")
}

/// The line under "Updates" in Settings.
pub fn update_status(update: &UpdateView) -> String {
    let current = format!("Version {}", update.current_version);
    match &update.state {
        UpdateState::Idle => current,
        UpdateState::Checking => format!("{current} · Checking for updates…"),
        UpdateState::UpToDate => format!("{current} · Up to date"),
        UpdateState::Available { version } => format!("{current} · {version} is available"),
        UpdateState::Installing { version, percent } => installing(version, *percent),
        UpdateState::Failed { message } => message.clone(),
    }
}

/// `Downloading Solidbar 0.2.0… 42%`
pub fn installing(version: &str, percent: Option<u8>) -> String {
    match percent {
        Some(percent) => format!("Downloading Solidbar {version}… {percent}%"),
        None => format!("Downloading Solidbar {version}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_durations() {
        assert_eq!(spoken_duration(0), "0 minutes");
        assert_eq!(spoken_duration(60), "1 minute");
        assert_eq!(spoken_duration(3600), "1 hour");
        assert_eq!(spoken_duration(5400), "1 hour 30 minutes");
        assert_eq!(spoken_duration(7_260), "2 hours 1 minute");
    }

    #[test]
    fn update_statuses() {
        let view = |state| UpdateView {
            current_version: "0.1.0".into(),
            state,
        };
        assert_eq!(update_status(&view(UpdateState::Idle)), "Version 0.1.0");
        assert_eq!(
            update_status(&view(UpdateState::Available {
                version: "0.2.0".into()
            })),
            "Version 0.1.0 · 0.2.0 is available"
        );
        assert_eq!(
            update_status(&view(UpdateState::Installing {
                version: "0.2.0".into(),
                percent: Some(42)
            })),
            "Downloading Solidbar 0.2.0… 42%"
        );
    }
}
