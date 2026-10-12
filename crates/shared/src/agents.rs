//! Connecting AI agents to Solidbar's MCP server (Settings › AI agents): what the agents'
//! configurations need, worded the way Settings copies it.

/// The server's name in the agents' configurations.
pub const SERVER_NAME: &str = "solidbar";

/// The argument that makes Solidbar's binary relay MCP over stdio instead of starting the app.
pub const BRIDGE_ARG: &str = "mcp";

/// `http://127.0.0.1:52731/mcp`
pub fn server_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

/// Adds Solidbar to Claude Code for all projects.
pub fn claude_code_command(url: &str, key: &str) -> String {
    format!(
        "claude mcp add --transport http --scope user {SERVER_NAME} {url} --header \"Authorization: Bearer {key}\""
    )
}

/// The `mcpServers` entry for agents that start their servers themselves, like Claude
/// Desktop: Solidbar's binary relays to the running app, and finds its address and key on
/// its own, so a new key or port doesn't break it.
pub fn desktop_config(bridge: &str) -> String {
    format!(
        "{{\n  \"mcpServers\": {{\n    \"{SERVER_NAME}\": {{\n      \"command\": {},\n      \"args\": [\"{BRIDGE_ARG}\"]\n    }}\n  }}\n}}",
        json_string(bridge)
    )
}

fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            character if character.is_control() => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(character)));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_gets_the_address_and_the_key() {
        assert_eq!(
            claude_code_command(&server_url(52731), "k3y"),
            "claude mcp add --transport http --scope user solidbar http://127.0.0.1:52731/mcp \
             --header \"Authorization: Bearer k3y\""
        );
    }

    #[test]
    fn the_desktop_config_is_json_that_starts_the_bridge() {
        let config = desktop_config("/Applications/Solidbar \"Beta\".app/Contents/MacOS/solidbar");
        let parsed: serde_json::Value = serde_json::from_str(&config).unwrap();
        let server = &parsed["mcpServers"]["solidbar"];
        assert_eq!(
            server["command"],
            "/Applications/Solidbar \"Beta\".app/Contents/MacOS/solidbar"
        );
        assert_eq!(server["args"], serde_json::json!(["mcp"]));
    }
}
