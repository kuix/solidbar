//! The project/task pairs offered as one-keystroke shortcuts (⌘1–⌘5) in the entry form.

use serde::{Deserialize, Serialize};

use super::entry::TimeEntry;

pub const LIMIT: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkPair {
    pub project_id: String,
    pub task_id: Option<String>,
}

impl WorkPair {
    pub fn of(entry: &TimeEntry) -> Option<WorkPair> {
        Some(WorkPair {
            project_id: entry.project_id.clone()?,
            task_id: entry.task_id.clone(),
        })
    }
}

/// Moves `pair` to the front, keeping at most [`LIMIT`] distinct pairs.
pub fn record(recents: &mut Vec<WorkPair>, pair: WorkPair) {
    recents.retain(|existing| *existing != pair);
    recents.insert(0, pair);
    recents.truncate(LIMIT);
}

/// Folds freshly loaded entries into the stored list: pairs used most recently come first.
pub fn merge_entries(recents: &[WorkPair], entries: &[TimeEntry]) -> Vec<WorkPair> {
    let mut newest_first: Vec<&TimeEntry> = entries.iter().collect();
    newest_first.sort_by_key(|entry| std::cmp::Reverse(entry.start));
    let mut merged = Vec::with_capacity(LIMIT);
    for pair in newest_first
        .into_iter()
        .filter_map(WorkPair::of)
        .chain(recents.iter().cloned())
    {
        if merged.len() == LIMIT {
            break;
        }
        if !merged.contains(&pair) {
            merged.push(pair);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testing::finished;

    fn pair(project: &str, task: Option<&str>) -> WorkPair {
        WorkPair {
            project_id: project.to_owned(),
            task_id: task.map(str::to_owned),
        }
    }

    fn on(project: Option<&str>, task: Option<&str>, start: &str) -> TimeEntry {
        TimeEntry {
            project_id: project.map(str::to_owned),
            task_id: task.map(str::to_owned),
            ..finished("x", start, start)
        }
    }

    #[test]
    fn recording_moves_a_pair_to_the_front_without_duplicates() {
        let mut recents = vec![pair("a", None), pair("b", Some("t")), pair("c", None)];
        record(&mut recents, pair("b", Some("t")));
        assert_eq!(
            recents,
            [pair("b", Some("t")), pair("a", None), pair("c", None)]
        );
    }

    #[test]
    fn keeps_at_most_five() {
        let mut recents = Vec::new();
        for project in ["a", "b", "c", "d", "e", "f"] {
            record(&mut recents, pair(project, None));
        }
        assert_eq!(recents.len(), LIMIT);
        assert_eq!(recents[0], pair("f", None));
        assert!(!recents.contains(&pair("a", None)));
    }

    #[test]
    fn merging_puts_the_newest_entries_first_and_skips_entries_without_project() {
        let stored = vec![pair("old", None), pair("a", None)];
        let entries = [
            on(Some("a"), None, "2026-10-08T07:00:00Z"),
            on(None, None, "2026-10-09T09:00:00Z"),
            on(Some("b"), Some("t"), "2026-10-09T08:00:00Z"),
        ];
        assert_eq!(
            merge_entries(&stored, &entries),
            [pair("b", Some("t")), pair("a", None), pair("old", None)]
        );
    }
}
