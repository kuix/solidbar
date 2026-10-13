//! The project and task picker's list: what matches the search, grouped under clients and
//! projects, so a row names only its task and the long names fit.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::fuzzy;
use crate::view::WorkOption;

/// How many matches are listed at most.
pub const LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    /// A client's name over its projects; `None` over the projects without a client. There
    /// are no headings when no project has a client.
    Client(Option<String>),
    /// Sets "No project" and "Break" apart from the projects.
    Divider,
    /// The `position`th of [`List::options`]. A task sits under its project.
    Option {
        position: usize,
        option: WorkOption,
        task: bool,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct List {
    pub rows: Vec<Row>,
    /// The options top to bottom: what ↑ and ↓ move through.
    pub options: Vec<WorkOption>,
    /// Where in `options` the best match is, for the highlight to start on.
    pub best: Option<usize>,
    /// How many options match. A project is also listed when only its tasks do.
    pub matched: usize,
}

struct Project {
    head: usize,
    tasks: Vec<usize>,
}

struct Section<'a> {
    client: Option<&'a str>,
    best: u32,
    projects: Vec<Project>,
}

/// Lists the options matching `query`. Clients with better matches come first, then by name,
/// and the projects without a client after them; projects and tasks keep the catalog's
/// order among equal matches. "No project" and "Break" come last.
pub fn list(query: &str, options: &[WorkOption]) -> List {
    let scores: Vec<Option<u32>> = options
        .iter()
        .map(|option| match &option.client_name {
            Some(client) => fuzzy::score(query, &format!("{} {client}", option.label)),
            None => fuzzy::score(query, &option.label),
        })
        .collect();
    let mut matching: Vec<usize> = (0..options.len())
        .filter(|&index| scores[index].is_some())
        .collect();
    matching.sort_by_key(|&index| Reverse(scores[index]));
    matching.truncate(LIMIT);

    let heads: HashMap<&str, usize> = options
        .iter()
        .enumerate()
        .filter(|(_, option)| option.task_id.is_none())
        .filter_map(|(index, option)| Some((option.project_id.as_deref()?, index)))
        .collect();
    let mut sections: Vec<Section> = Vec::new();
    let mut others = Vec::new();
    // Best matches first, so each client and project is placed by its best match.
    for &index in &matching {
        let option = &options[index];
        let Some(project_id) = option.project_id.as_deref() else {
            others.push(index);
            continue;
        };
        let head = match option.task_id {
            Some(_) => heads.get(project_id).copied().unwrap_or(index),
            None => index,
        };
        let client = options[head].client_name.as_deref();
        let section = match sections.iter().position(|section| section.client == client) {
            Some(position) => &mut sections[position],
            None => {
                sections.push(Section {
                    client,
                    best: scores[index].unwrap_or_default(),
                    projects: Vec::new(),
                });
                sections.last_mut().expect("just added")
            }
        };
        let project = match section
            .projects
            .iter()
            .position(|project| project.head == head)
        {
            Some(position) => &mut section.projects[position],
            None => {
                section.projects.push(Project {
                    head,
                    tasks: Vec::new(),
                });
                section.projects.last_mut().expect("just added")
            }
        };
        if index != head {
            project.tasks.push(index);
        }
    }
    sections.sort_by_cached_key(|section| {
        (
            Reverse(section.best),
            section.client.is_none(),
            section.client.map(str::to_lowercase),
        )
    });

    let mut list = List {
        matched: matching.len(),
        ..List::default()
    };
    // The catalog index of each of `list.options`.
    let mut listed = Vec::new();
    let mut add = |list: &mut List, index: usize, task: bool| {
        list.rows.push(Row::Option {
            position: list.options.len(),
            option: options[index].clone(),
            task,
        });
        list.options.push(options[index].clone());
        listed.push(index);
    };
    let headings = sections.iter().any(|section| section.client.is_some());
    for section in &sections {
        if headings {
            list.rows
                .push(Row::Client(section.client.map(str::to_owned)));
        }
        for project in &section.projects {
            add(&mut list, project.head, false);
            for &task in &project.tasks {
                add(&mut list, task, true);
            }
        }
    }
    if !list.rows.is_empty() && !others.is_empty() {
        list.rows.push(Row::Divider);
    }
    for &other in &others {
        add(&mut list, other, false);
    }

    let top = matching.first().map(|&index| scores[index]);
    list.best = top.and_then(|top| listed.iter().position(|&index| scores[index] == top));
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::EntryKind;

    fn project(id: &str, name: &str, client: Option<&str>) -> WorkOption {
        WorkOption {
            kind: EntryKind::Work,
            project_id: Some(id.to_owned()),
            task_id: None,
            label: name.to_owned(),
            task_name: None,
            client_name: client.map(str::to_owned),
            color: Some("#ef5350".to_owned()),
            billable: true,
        }
    }

    fn task(project: &WorkOption, id: &str, name: &str) -> WorkOption {
        WorkOption {
            task_id: Some(id.to_owned()),
            label: format!("{} › {name}", project.label),
            task_name: Some(name.to_owned()),
            ..project.clone()
        }
    }

    fn no_project() -> WorkOption {
        WorkOption {
            kind: EntryKind::Work,
            project_id: None,
            task_id: None,
            label: "No project".to_owned(),
            task_name: None,
            client_name: None,
            color: None,
            billable: false,
        }
    }

    /// The catalog as the backend sends it: projects by name, each followed by its tasks.
    fn catalog() -> Vec<WorkOption> {
        let admin = project("p1", "Admin", None);
        let allee = project("p2", "Allee Support", Some("Allee Center"));
        let lavylites = project("p3", "Lavylites Support", Some("Lavylites"));
        let webshop = project("p4", "Webshop", Some("Lavylites"));
        vec![
            admin.clone(),
            allee.clone(),
            task(&allee, "t1", "Content Management"),
            task(&allee, "t2", "Deployment"),
            lavylites.clone(),
            task(&lavylites, "t3", "Frontend development"),
            webshop,
            no_project(),
        ]
    }

    fn rows(list: &List) -> Vec<String> {
        list.rows
            .iter()
            .map(|row| match row {
                Row::Client(Some(client)) => format!("# {client}"),
                Row::Client(None) => "# No client".to_owned(),
                Row::Divider => "---".to_owned(),
                Row::Option { option, task, .. } => match (task, &option.task_name) {
                    (true, Some(name)) => format!("  {name}"),
                    _ => option.label.clone(),
                },
            })
            .collect()
    }

    #[test]
    fn everything_is_grouped_under_clients_by_name() {
        let list = list("", &catalog());
        assert_eq!(
            rows(&list),
            [
                "# Allee Center",
                "Allee Support",
                "  Content Management",
                "  Deployment",
                "# Lavylites",
                "Lavylites Support",
                "  Frontend development",
                "Webshop",
                "# No client",
                "Admin",
                "---",
                "No project",
            ]
        );
        assert_eq!(list.options.len(), 8);
        assert_eq!(list.matched, 8);
        assert_eq!(list.best, Some(0));
    }

    #[test]
    fn a_task_is_listed_under_its_project_and_the_highlight_starts_on_it() {
        let list = list("deploy", &catalog());
        assert_eq!(
            rows(&list),
            ["# Allee Center", "Allee Support", "  Deployment"]
        );
        assert_eq!(list.matched, 1);
        assert_eq!(list.best, Some(1));
        assert_eq!(list.options[1].task_id.as_deref(), Some("t2"));
    }

    #[test]
    fn the_client_with_the_best_match_comes_first() {
        let options = vec![
            project("p1", "Support", Some("Acme")),
            project("p2", "Portal", Some("Zed")),
        ];
        // "port" starts a word in Portal, and is only inside one in Support.
        assert_eq!(
            rows(&list("port", &options)),
            ["# Zed", "Portal", "# Acme", "Support"]
        );

        // A client's name finds all its projects.
        assert_eq!(
            rows(&list("lavylites", &catalog())),
            [
                "# Lavylites",
                "Lavylites Support",
                "  Frontend development",
                "Webshop",
            ]
        );
    }

    #[test]
    fn without_clients_there_are_no_headings() {
        let admin = project("p1", "Admin", None);
        let options = vec![admin.clone(), task(&admin, "t1", "Email"), no_project()];
        assert_eq!(
            rows(&list("", &options)),
            ["Admin", "  Email", "---", "No project"]
        );
        assert_eq!(rows(&list("no", &options)), ["No project"]);
    }

    #[test]
    fn nothing_matching_lists_nothing() {
        let list = list("zzz", &catalog());
        assert_eq!(list, List::default());
    }
}
