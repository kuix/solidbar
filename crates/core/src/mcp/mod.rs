//! The MCP server for AI agents: the Model Context Protocol over Streamable HTTP, on the
//! loopback interface only, behind a key. Its tools ([`tools`]) read projects and time
//! entries, add entries and change them, through the engine and its rules.
//!
//! Agents that only start servers themselves, like Claude Desktop, reach it through
//! [`bridge`]: Solidbar's binary relaying stdio to the running app.

pub mod bridge;
mod controller;
mod http;
mod protocol;
mod tools;

pub use controller::AgentServer;
