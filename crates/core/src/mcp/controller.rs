use std::sync::Arc;

use solidbar_shared::agents::server_url;
use solidbar_shared::view::AgentServerView;
use tokio::sync::Mutex;

use super::http::Server;
use crate::engine::Engine;

/// Runs the MCP server while Settings says so, and tells Settings how it went.
pub struct AgentServer {
    engine: Arc<Engine>,
    /// The binary agents start to reach the server over stdio (see [`super::bridge`]).
    bridge: String,
    running: Mutex<Option<Server>>,
}

impl AgentServer {
    pub fn new(engine: Arc<Engine>, bridge: String) -> Arc<Self> {
        Arc::new(Self {
            engine,
            bridge,
            running: Mutex::new(None),
        })
    }

    /// Starts, stops or moves the server to match the settings.
    pub async fn sync(&self) {
        let access = self.engine.agent_access();
        let wanted = access.port.filter(|_| access.enabled);
        let mut running = self.running.lock().await;
        if let (Some(server), Some(port)) = (running.as_ref(), wanted)
            && server.port == port
        {
            self.report_running(port);
            return;
        }
        if let Some(server) = running.take() {
            server.stop().await;
        }
        let Some(port) = wanted else {
            self.engine.set_agent_server(AgentServerView::Off);
            return;
        };
        match Server::start(self.engine.clone(), port).await {
            Ok(server) => {
                *running = Some(server);
                self.report_running(port);
            }
            Err(error) => {
                log::warn!("the MCP server couldn't start on port {port}: {error}");
                let message = if error.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "Another app is using port {port}. Move Solidbar to another port, then give your agents the new address."
                    )
                } else {
                    format!("The MCP server couldn't start: {error}")
                };
                self.engine
                    .set_agent_server(AgentServerView::Failed { message });
            }
        }
    }

    fn report_running(&self, port: u16) {
        self.engine.set_agent_server(AgentServerView::Running {
            url: server_url(port),
            bridge: self.bridge.clone(),
        });
    }
}
