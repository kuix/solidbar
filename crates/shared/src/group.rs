//! A day's entries as the list shows them: entries with the same project, task, notes,
//! billable flag and kind form one row, like solidtime's own time entry list. Continuing an
//! entry creates a new one in solidtime, so this is what makes it look continued.

use crate::view::{EntryKind, EntryView};

/// One row of the day: one or more entries, in the order they started.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryGroup {
    pub entries: Vec<EntryView>,
}

impl EntryGroup {
    /// The id of its first entry, which stays put while the group grows.
    pub fn id(&self) -> &str {
        &self.first().id
    }

    /// Project, task, notes and the rest are the same for every entry.
    pub fn first(&self) -> &EntryView {
        &self.entries[0]
    }

    /// The entry that −/+ and the timer act on: the running one, else the one that started last.
    pub fn latest(&self) -> &EntryView {
        self.entries
            .iter()
            .find(|entry| entry.running)
            .unwrap_or_else(|| &self.entries[self.entries.len() - 1])
    }

    pub fn ids(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.id.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_break(&self) -> bool {
        self.first().kind == EntryKind::Break
    }

    pub fn duration_seconds(&self) -> i64 {
        self.entries
            .iter()
            .map(|entry| entry.duration_seconds)
            .sum()
    }

    pub fn running(&self) -> bool {
        self.entries.iter().any(|entry| entry.running)
    }

    pub fn pending(&self) -> bool {
        self.entries.iter().any(|entry| entry.pending)
    }

    /// When the first entry started.
    pub fn start_label(&self) -> &str {
        &self.first().start_label
    }

    /// When the last entry ended; `None` while one is running.
    pub fn end_label(&self) -> Option<&str> {
        if self.running() {
            return None;
        }
        self.entries
            .last()
            .and_then(|entry| entry.end_label.as_deref())
    }
}

/// Groups entries (ordered by start) that share project, task, notes, billable and kind. A group
/// sits where its first entry started.
pub fn group_entries(entries: &[EntryView]) -> Vec<EntryGroup> {
    let mut groups: Vec<EntryGroup> = Vec::new();
    for entry in entries {
        match groups
            .iter_mut()
            .find(|group| same_work(group.first(), entry))
        {
            Some(group) => group.entries.push(entry.clone()),
            None => groups.push(EntryGroup {
                entries: vec![entry.clone()],
            }),
        }
    }
    groups
}

fn same_work(a: &EntryView, b: &EntryView) -> bool {
    a.kind == b.kind
        && a.project_id == b.project_id
        && a.task_id == b.task_id
        && a.billable == b.billable
        && a.description == b.description
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, description: &str, seconds: i64) -> EntryView {
        EntryView {
            id: id.to_owned(),
            kind: EntryKind::Work,
            project_id: Some("p1".to_owned()),
            task_id: None,
            project_name: Some("Design system".to_owned()),
            task_name: None,
            client_name: None,
            color: None,
            description: description.to_owned(),
            billable: true,
            start_label: format!("start {id}"),
            end_label: Some(format!("end {id}")),
            duration_seconds: seconds,
            running: false,
            pending: false,
        }
    }

    #[test]
    fn continued_entries_share_a_row_where_the_first_one_started() {
        let entries = [
            entry("a", "Header", 7200),
            entry("b", "Call", 600),
            entry("c", "Header", 180),
        ];
        let groups = group_entries(&entries);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].ids(), ["a", "c"]);
        assert_eq!(groups[0].id(), "a");
        assert_eq!(groups[0].duration_seconds(), 7380);
        assert_eq!(groups[0].start_label(), "start a");
        assert_eq!(groups[0].end_label(), Some("end c"));
        assert_eq!(groups[1].ids(), ["b"]);
    }

    #[test]
    fn any_difference_keeps_entries_apart() {
        let base = entry("a", "Header", 60);
        let variants = [
            EntryView {
                kind: EntryKind::Break,
                ..entry("b", "Header", 60)
            },
            EntryView {
                project_id: None,
                ..entry("c", "Header", 60)
            },
            EntryView {
                task_id: Some("t1".to_owned()),
                ..entry("d", "Header", 60)
            },
            EntryView {
                billable: false,
                ..entry("e", "Header", 60)
            },
            entry("f", "header", 60),
        ];
        for variant in variants {
            assert_eq!(group_entries(&[base.clone(), variant]).len(), 2);
        }
    }

    #[test]
    fn a_running_entry_makes_the_group_running_and_is_the_one_to_act_on() {
        let entries = [
            entry("a", "Header", 7200),
            EntryView {
                running: true,
                end_label: None,
                ..entry("b", "Header", 60)
            },
            entry("c", "Header", 60),
        ];
        let group = &group_entries(&entries)[0];
        assert!(group.running());
        assert_eq!(group.latest().id, "b");
        assert_eq!(group.end_label(), None);
    }
}
