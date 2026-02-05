//! ACP (Agent Client Protocol) router for multi-agent chat.
//!
//! Replaces Claude-specific subprocess spawning with a generic JSON-RPC 2.0
//! over stdio transport that supports Claude Code, Gemini CLI, Copilot CLI,
//! and Codex CLI.

mod process;
mod registry;
pub mod router;
mod transport;
mod types;

pub use router::AcpRouter;
