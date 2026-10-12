//! The tools agents get: their descriptions and input schemas, and the calls into the engine.
//!
//! Results come back as JSON, both structured and as text. Mistakes an agent can fix (a wrong
//! date, an unknown project) come back as tool errors it can read, not protocol errors.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::protocol::INVALID_PARAMS;
use crate::engine::Engine;
use crate::engine::agent::{EntryChanges, MAX_NEW_ENTRIES, NewEntry};

const DATE: &str = "YYYY-MM-DD";
const START_TIME: &str = "HH:MM, 24-hour. Leave it out to start where the day's last entry ends \
(at the start of the workday on an empty day).";
const DURATION: &str = "How long: \"1:30\", \"1.5\", \"90m\" or \"1h30m\"; a number means hours. \
Give this or end_time.";
const END_TIME: &str = "HH:MM, 24-hour, on the same day. Give this or duration.";

pub fn list() -> Value {
    json!({ "tools": [
        {
            "name": "list_projects",
            "title": "List projects",
            "description": "Lists the projects of the organization chosen in Solidbar, with \
    their clients and tasks, and whether their entries are billable by default. Use the ids to \
    add or change time entries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_archived": {
                        "type": "boolean",
                        "description": "Also list archived projects, which take no new entries. Default: false.",
                    },
                },
                "additionalProperties": false,
            },
            "annotations": read_only("List projects"),
        },
        {
            "name": "list_time_entries",
            "title": "List time entries",
            "description": "Lists the person's time entries on a range of days, oldest first: \
    times, duration, project, task, client, description and billable, with the total of work. \
    Use it to see what's tracked before adding entries, and to find the ids of entries to change.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": format!("First day, {DATE}. Default: today, or `to` when that's earlier."),
                    },
                    "to": {
                        "type": "string",
                        "description": format!("Last day, included, {DATE}. Default: today, or `from` when that's later."),
                    },
                },
                "additionalProperties": false,
            },
            "annotations": read_only("List time entries"),
        },
        {
            "name": "add_time_entries",
            "title": "Add time entries",
            "description": "Adds time entries to the person's solidtime, in the order given. \
    An entry without start_time starts where the day's last entry ends, so \"two hours on X on \
    Monday\" needs only date, duration and project_id. If an entry doesn't make sense, none is \
    added, and the answer says what to fix in which entry.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entries": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_NEW_ENTRIES,
                        "items": {
                            "type": "object",
                            "properties": {
                                "date": { "type": "string", "description": DATE },
                                "start_time": { "type": "string", "description": START_TIME },
                                "duration": { "type": ["string", "number"], "description": DURATION },
                                "end_time": { "type": "string", "description": END_TIME },
                                "project_id": { "type": "string", "description": "From list_projects." },
                                "task_id": {
                                    "type": "string",
                                    "description": "From list_projects. The task's project comes with it.",
                                },
                                "description": { "type": "string", "description": "What was done." },
                                "billable": {
                                    "type": "boolean",
                                    "description": "Default: what the project says.",
                                },
                            },
                            "required": ["date"],
                            "additionalProperties": false,
                        },
                    },
                },
                "required": ["entries"],
                "additionalProperties": false,
            },
            "annotations": {
                "title": "Add time entries",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false,
            },
        },
        {
            "name": "update_time_entry",
            "title": "Change a time entry",
            "description": "Changes a time entry found with list_time_entries (or added with \
    add_time_entries): its day, times, project, task, description or billable flag. What's left \
    out stays as it is; a new start keeps the duration unless duration or end_time is given too. \
    A running timer has no end to set.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The entry's id." },
                    "date": { "type": "string", "description": format!("Move it to this day, {DATE}.") },
                    "start_time": { "type": "string", "description": "HH:MM, 24-hour." },
                    "duration": { "type": ["string", "number"], "description": DURATION },
                    "end_time": { "type": "string", "description": END_TIME },
                    "project_id": {
                        "type": ["string", "null"],
                        "description": "From list_projects; null removes the project and its task.",
                    },
                    "task_id": {
                        "type": ["string", "null"],
                        "description": "From list_projects; null removes the task. The task's project comes with it.",
                    },
                    "description": { "type": "string" },
                    "billable": { "type": "boolean" },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": {
                "title": "Change a time entry",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": false,
            },
        },
    ]})
}

/// Runs a tool; `Err` only when the call itself is malformed (no such tool).
pub async fn call(engine: &Engine, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "Which tool? `name` is missing".to_owned()))?;
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(arguments) => arguments.clone(),
    };
    let outcome = match name {
        "list_projects" => list_projects(engine, arguments).await,
        "list_time_entries" => list_time_entries(engine, arguments).await,
        "add_time_entries" => add_time_entries(engine, arguments).await,
        "update_time_entry" => update_time_entry(engine, arguments).await,
        name => return Err((INVALID_PARAMS, format!("There's no tool {name}"))),
    };
    Ok(match outcome {
        Ok(result) => json!({
            "content": [{ "type": "text", "text": result.to_string() }],
            "structuredContent": result,
            "isError": false,
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true,
        }),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectsArgs {
    #[serde(default)]
    include_archived: bool,
}

async fn list_projects(engine: &Engine, arguments: Value) -> Result<Value, String> {
    let args: ProjectsArgs = parse(arguments)?;
    let listing = engine
        .agent_projects(args.include_archived)
        .await
        .map_err(|error| error.message)?;
    Ok(json!(listing))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntriesArgs {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

async fn list_time_entries(engine: &Engine, arguments: Value) -> Result<Value, String> {
    let args: EntriesArgs = parse(arguments)?;
    let listing = engine
        .agent_entries(args.from.as_deref(), args.to.as_deref())
        .await
        .map_err(|error| error.message)?;
    Ok(json!(listing))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    entries: Vec<NewEntry>,
}

async fn add_time_entries(engine: &Engine, arguments: Value) -> Result<Value, String> {
    let args: AddArgs = parse(arguments)?;
    match engine.agent_add_entries(&args.entries).await {
        Ok(added) => Ok(json!(added)),
        Err(failure) if failure.added.is_empty() => Err(failure.message),
        Err(failure) => Err(format!(
            "{}\nAdded: {}",
            failure.message,
            json!(failure.added)
        )),
    }
}

async fn update_time_entry(engine: &Engine, arguments: Value) -> Result<Value, String> {
    let changes: EntryChanges = parse(arguments)?;
    let updated = engine
        .agent_update_entry(&changes)
        .await
        .map_err(|error| error.message)?;
    Ok(json!(updated))
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| format!("Invalid arguments: {error}"))
}

fn read_only(title: &str) -> Value {
    json!({
        "title": title,
        "readOnlyHint": true,
        "openWorldHint": false,
    })
}
