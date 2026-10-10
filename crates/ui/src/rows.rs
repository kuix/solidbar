//! The rows of the day list: a group of continued entries, and (once opened with →) each of
//! its entries on a row of its own.

use std::collections::HashSet;

use solidbar_shared::group::EntryGroup;
use solidbar_shared::view::EntryView;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Group(EntryGroup),
    /// One entry of an opened group.
    Interval {
        group: EntryGroup,
        entry: EntryView,
    },
}

impl Row {
    /// The id of the row's focusable element.
    pub fn dom_id(&self) -> String {
        match self {
            Row::Group(group) => group_dom_id(group),
            Row::Interval { entry, .. } => interval_dom_id(entry),
        }
    }

    pub fn group(&self) -> &EntryGroup {
        match self {
            Row::Group(group) | Row::Interval { group, .. } => group,
        }
    }

    /// The entry the timer button and −/+ act on.
    pub fn target(&self) -> &EntryView {
        match self {
            Row::Group(group) => group.latest(),
            Row::Interval { entry, .. } => entry,
        }
    }

    /// What deleting the row deletes.
    pub fn ids(&self) -> Vec<String> {
        match self {
            Row::Group(group) => group.ids(),
            Row::Interval { entry, .. } => vec![entry.id.clone()],
        }
    }

    pub fn running(&self) -> bool {
        match self {
            Row::Group(group) => group.running(),
            Row::Interval { entry, .. } => entry.running,
        }
    }

    pub fn pending(&self) -> bool {
        match self {
            Row::Group(group) => group.pending(),
            Row::Interval { entry, .. } => entry.pending,
        }
    }

    pub fn is_break(&self) -> bool {
        self.group().is_break()
    }
}

pub fn group_dom_id(group: &EntryGroup) -> String {
    format!("entry-{}", group.id())
}

pub fn interval_dom_id(entry: &EntryView) -> String {
    format!("interval-{}", entry.id)
}

/// The rows on screen, top to bottom: every group, and the entries of the opened ones.
pub fn visible_rows(groups: &[EntryGroup], expanded: &HashSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    for group in groups {
        rows.push(Row::Group(group.clone()));
        if group.len() > 1 && expanded.contains(group.id()) {
            rows.extend(group.entries.iter().map(|entry| Row::Interval {
                group: group.clone(),
                entry: entry.clone(),
            }));
        }
    }
    rows
}
