//! Text for durations and entries, on screen and for VoiceOver.

use solidbar_shared::group::EntryGroup;
use solidbar_shared::update::{UpdateState, UpdateView};
use solidbar_shared::view::{EntryKind, EntryView};

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

/// `Design system › Header`, `No project`, or `Break`.
pub fn work_title(entry: &EntryView) -> String {
    if entry.kind == EntryKind::Break {
        return "Break".to_owned();
    }
    match (&entry.project_name, &entry.task_name) {
        (Some(project), Some(task)) => format!("{project} › {task}"),
        (Some(project), None) => project.clone(),
        (None, _) if entry.project_id.is_some() => "Unknown project".to_owned(),
        (None, _) => "No project".to_owned(),
    }
}

/// Everything a screen reader needs about a row in one sentence.
pub fn group_label(group: &EntryGroup) -> String {
    let entry = group.first();
    let mut parts = vec![work_title(entry)];
    if let Some(client) = &entry.client_name {
        parts.push(client.clone());
    }
    if !entry.description.is_empty() {
        parts.push(entry.description.clone());
    }
    let until = group.end_label().unwrap_or("now");
    let mut times = spoken_duration(group.duration_seconds());
    if group.len() > 1 {
        times.push_str(&format!(", {} entries", group.len()));
    }
    parts.push(format!("{times}, {} to {until}", group.start_label()));
    if group.running() {
        parts.push(
            if group.is_break() {
                "on a break now"
            } else {
                "timer running"
            }
            .to_owned(),
        );
    }
    if entry.billable {
        parts.push("billable".to_owned());
    }
    if group.pending() {
        parts.push("saving".to_owned());
    }
    parts.join(". ")
}

/// One entry of an opened group: `09:00 to 11:00, 2 hours`.
pub fn interval_label(entry: &EntryView) -> String {
    let until = entry.end_label.as_deref().unwrap_or("now");
    let mut label = format!(
        "{} to {until}, {}",
        entry.start_label,
        spoken_duration(entry.duration_seconds)
    );
    if entry.running {
        label.push_str(", timer running");
    }
    if entry.pending {
        label.push_str(", saving");
    }
    label
}

/// `09:00–11:00`, or `09:00–now`.
pub fn time_range(start: &str, end: Option<&str>) -> String {
    format!("{start}–{}", end.unwrap_or("now"))
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

    fn entry(id: &str, seconds: i64) -> EntryView {
        EntryView {
            id: id.to_owned(),
            kind: EntryKind::Work,
            project_id: Some("p1".to_owned()),
            task_id: None,
            project_name: Some("Design system".to_owned()),
            task_name: None,
            client_name: Some("Kuix".to_owned()),
            color: None,
            description: "Header".to_owned(),
            billable: true,
            start_label: "09:00".to_owned(),
            end_label: Some("11:00".to_owned()),
            duration_seconds: seconds,
            running: false,
            pending: false,
        }
    }

    #[test]
    fn group_labels_sum_up_their_entries() {
        let group = EntryGroup {
            entries: vec![
                entry("a", 7200),
                EntryView {
                    start_label: "11:05".to_owned(),
                    end_label: None,
                    running: true,
                    ..entry("b", 180)
                },
            ],
        };
        assert_eq!(
            group_label(&group),
            "Design system. Kuix. Header. 2 hours 3 minutes, 2 entries, 09:00 to now. \
             timer running. billable"
        );
        assert_eq!(
            interval_label(&group.entries[1]),
            "11:05 to now, 3 minutes, timer running"
        );
        let pause = EntryGroup {
            entries: vec![EntryView {
                kind: EntryKind::Break,
                project_id: None,
                project_name: None,
                client_name: None,
                description: String::new(),
                billable: false,
                ..entry("c", 900)
            }],
        };
        assert_eq!(group_label(&pause), "Break. 15 minutes, 09:00 to 11:00");
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
