//! Agent spawn configurations.
//!
//! Maps `AgentType` to the command and arguments needed to spawn each agent
//! in ACP mode.

/// Spawn configuration for an ACP agent.
pub struct AgentSpawnConfig {
    pub command: &'static str,
    pub args: Vec<String>,
}

/// Get the spawn configuration for an agent by name.
///
/// Returns `None` if the agent name is not recognized.
pub fn spawn_config(agent: &str) -> Option<AgentSpawnConfig> {
    match agent {
        "claude" => Some(AgentSpawnConfig {
            command: "claude-code-acp",
            args: vec![],
        }),
        "gemini" => Some(AgentSpawnConfig {
            command: "gemini",
            args: vec!["--experimental-acp".into()],
        }),
        "copilot" => Some(AgentSpawnConfig {
            command: "copilot",
            args: vec!["--acp".into()],
        }),
        "codex" => Some(AgentSpawnConfig {
            command: "codex-acp",
            args: vec![],
        }),
        _ => None,
    }
}
