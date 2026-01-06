//! MCP (Model Context Protocol) server implementation for Bobbin.
//!
//! This module exposes Bobbin's code search and analysis capabilities via the
//! Model Context Protocol, allowing AI agents (Claude, Cursor) to use Bobbin as a tool.

mod server;
mod tools;

pub use server::run_server;
