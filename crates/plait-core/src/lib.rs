//! Headless models, client state, local host transport, and read-only Git access.
//!
//! Blocking Git and host operations must run on the caller's background executor.

pub mod fixtures;
pub mod git;
pub mod host;
pub mod host_state;
pub mod local_socket;
pub mod model;
pub mod state;
