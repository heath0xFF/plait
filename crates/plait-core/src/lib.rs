//! Headless models, client state, local host transport, and read-only Git access.
//!
//! Blocking Git and host operations must run on the caller's background executor.

pub mod git;
pub mod host;
pub mod host_state;
pub mod local_socket;
pub mod machine;
pub mod markdown;
pub mod model;
pub mod preferences;
pub mod session;
pub mod state;
pub mod transcript;
pub mod update;
