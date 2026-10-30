//! The project and task picker's list: what matches the search, grouped under clients and
//! projects, so a row names only its task and the long names fit. Without a search, the
//! recently used choices come first, then the favorites.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::fuzzy;
use crate::view::{FavoriteView, WorkOption};

/// How many matches are listed at most.
pub const LIMIT: usize = 200;

/// How many recently used choices the list starts with; ⌘1–⌘3 pick them too.
pub const RECENTS: usize = 3;

/// How many favorites have a shortcut, ⌃1–⌃9.
pub const FAVORITE_KEYS: usize = 9;

/// What picking an option does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Chooses the project and task (or "No project", or "Break").
    Work(WorkOption),
    /// Fills the form in with a favorite: its project, task, notes, duration and billable.
    Favorite(FavoriteView),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    /// "Recently used", over the recent choices at the top.
    Recents,
    /// "Favorites", over the favorites after the recent choices.
    Favorites,
    /// A client's name over its projects; `None` over the projects without a client. There
    /// are no headings when no project has a client.
    Client(Option<String>),
    /// Sets "No project" and "Break" apart from the projects, and the recent choices from
    /// the projects when there are no client headings.
    Divider,
    /// The `position`th of [`List::options`]. A task sits under its project; a recent choice
    /// (`recent` is its ⌘ number) is named whole.
    Option {
        position: usize,
        option: WorkOption,
        task: bool,
        recent: Option<usize>,
    },
    /// The `position`th of [`List::options`], a favorite; `key` is its ⌃ number.
    Favorite {
        position: usize,
        favorite: FavoriteView,
        key: Option<usize>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct List {
    pub rows: Vec<Row>,
    /// The options top to bottom: what ↑ and ↓ move through.
    pub options: Vec<Choice>,
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
/// order among equal matches. "No project" and "Break" come last. Without a search, the
/// first [`RECENTS`] of `recents` and then the `favorites` come before everything, and the
/// highlight starts on top.
pub fn list(
    query: &str,
    options: &[WorkOption],
    recents: &[WorkOption],
    favorites: &[FavoriteView],
) -> List {
    let browsing = query.split_whitespace().next().is_none();
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
            recent: None,
        });
        list.options.push(Choice::Work(options[index].clone()));
        listed.push(index);
    };
    let (recents, favorites) = if browsing {
        (recents, favorites)
    } else {
        (&[][..], &[][..])
    };
    for (number, option) in recents.iter().take(RECENTS).enumerate() {
        if number == 0 {
            list.rows.push(Row::Recents);
        }
        list.rows.push(Row::Option {
            position: list.options.len(),
            option: option.clone(),
            task: false,
            recent: Some(number + 1),
        });
        list.options.push(Choice::Work(option.clone()));
    }
    for (number, favorite) in favorites.iter().enumerate() {
        if number == 0 {
            list.rows.push(Row::Favorites);
        }
        list.rows.push(Row::Favorite {
            position: list.options.len(),
            favorite: favorite.clone(),
            key: (number < FAVORITE_KEYS).then_some(number + 1),
        });
        list.options.push(Choice::Favorite(favorite.clone()));
    }
    let after_top = list.rows.len();
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
    if list.rows.len() > after_top && !others.is_empty() {
        list.rows.push(Row::Divider);
    }
    for &other in &others {
        add(&mut list, other, false);
    }
    // Without a client heading after them, a line ends the recent choices and favorites.
    let next = list.rows.get(after_top);
    if after_top > 0 && next.is_some_and(|row| !matches!(row, Row::Client(_))) {
        list.rows.insert(after_top, Row::Divider);
    }

    list.best = if browsing {
        (!list.options.is_empty()).then_some(0)
    } else {
        let top = matching.first().map(|&index| scores[index]);
        top.and_then(|top| listed.iter().position(|&index| scores[index] == top))
    };
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
                Row::Recents => "# Recently used".to_owned(),
                Row::Favorites => "# Favorites".to_owned(),
                Row::Favorite { favorite, key, .. } => match key {
                    Some(key) => format!("⌃{key} {}", favorite.description),
                    None => favorite.description.clone(),
                },
                Row::Client(Some(client)) => format!("# {client}"),
                Row::Client(None) => "# No client".to_owned(),
                Row::Divider => "---".to_owned(),
                Row::Option {
                    option,
                    task,
                    recent,
                    ..
                } => match (task, &option.task_name, recent) {
                    (_, _, Some(number)) => format!("⌘{number} {}", option.label),
                    (true, Some(name), _) => format!("  {name}"),
                    _ => option.label.clone(),
                },
            })
            .collect()
    }

    #[test]
    fn everything_is_grouped_under_clients_by_name() {
        let list = list("", &catalog(), &[], &[]);
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
        let list = list("deploy", &catalog(), &[], &[]);
        assert_eq!(
            rows(&list),
            ["# Allee Center", "Allee Support", "  Deployment"]
        );
        assert_eq!(list.matched, 1);
        assert_eq!(list.best, Some(1));
        assert!(
            matches!(&list.options[1], Choice::Work(option) if option.task_id.as_deref() == Some("t2"))
        );
    }

    #[test]
    fn the_client_with_the_best_match_comes_first() {
        let options = vec![
            project("p1", "Support", Some("Acme")),
            project("p2", "Portal", Some("Zed")),
        ];
        // "port" starts a word in Portal, and is only inside one in Support.
        assert_eq!(
            rows(&list("port", &options, &[], &[])),
            ["# Zed", "Portal", "# Acme", "Support"]
        );

        // A client's name finds all its projects.
        assert_eq!(
            rows(&list("lavylites", &catalog(), &[], &[])),
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
            rows(&list("", &options, &[], &[])),
            ["Admin", "  Email", "---", "No project"]
        );
        assert_eq!(rows(&list("no", &options, &[], &[])), ["No project"]);
    }

    #[test]
    fn without_a_search_the_list_starts_with_three_recent_choices() {
        let options = catalog();
        // Most recent first, as the backend sends them.
        let recents = [&options[5], &options[0], &options[2], &options[6]].map(Clone::clone);
        let list = list("", &options, &recents, &[]);
        assert_eq!(
            rows(&list)[..5],
            [
                "# Recently used",
                "⌘1 Lavylites Support › Frontend development",
                "⌘2 Admin",
                "⌘3 Allee Support › Content Management",
                "# Allee Center",
            ]
        );
        // The recent choices are listed in their places too, and the highlight starts on top.
        assert_eq!(list.options.len(), 3 + 8);
        assert_eq!(list.matched, 8);
        assert_eq!(list.best, Some(0));

        // A search lists only what matches, without the recent choices.
        assert_eq!(rows(&list_with("admin", &recents)), ["Admin"]);
    }

    #[test]
    fn without_client_headings_a_line_ends_the_recent_choices() {
        let admin = project("p1", "Admin", None);
        let options = vec![admin.clone(), task(&admin, "t1", "Email"), no_project()];
        assert_eq!(
            rows(&list("", &options, &[options[1].clone()], &[])),
            [
                "# Recently used",
                "⌘1 Admin › Email",
                "---",
                "Admin",
                "  Email",
                "---",
                "No project",
            ]
        );
    }

    fn favorite(option: &WorkOption, description: &str) -> FavoriteView {
        FavoriteView {
            option: option.clone(),
            description: description.to_owned(),
            duration: "1:00".to_owned(),
            billable: true,
        }
    }

    #[test]
    fn the_favorites_follow_the_recent_choices() {
        let options = catalog();
        let recents = [options[0].clone()];
        let favorites = [
            favorite(&options[2], "Weekly"),
            favorite(&options[6], "Standup"),
        ];
        let list = list("", &options, &recents, &favorites);
        assert_eq!(
            rows(&list)[..6],
            [
                "# Recently used",
                "⌘1 Admin",
                "# Favorites",
                "⌃1 Weekly",
                "⌃2 Standup",
                "# Allee Center",
            ]
        );
        assert_eq!(list.options.len(), 1 + 2 + 8);
        assert_eq!(list.options[1], Choice::Favorite(favorites[0].clone()));
        // The highlight still starts on the most recent choice.
        assert_eq!(list.best, Some(0));

        // A search lists only the projects and tasks that match.
        assert_eq!(
            rows(&super::list("weekly", &options, &recents, &favorites)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn without_client_headings_a_line_ends_the_favorites() {
        let admin = project("p1", "Admin", None);
        let options = vec![admin.clone(), no_project()];
        let favorites = [favorite(&admin, "Weekly")];
        assert_eq!(
            rows(&list("", &options, &[], &favorites)),
            [
                "# Favorites",
                "⌃1 Weekly",
                "---",
                "Admin",
                "---",
                "No project"
            ]
        );
    }

    fn list_with(query: &str, recents: &[WorkOption]) -> List {
        list(query, &catalog(), recents, &[])
    }

    #[test]
    fn nothing_matching_lists_nothing() {
        let list = list("zzz", &catalog(), &[], &[]);
        assert_eq!(list, List::default());
    }
}
