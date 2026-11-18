//! Headless models, client state, local host transport, and read-only Git access.
//!
//! Blocking Git and host operations must run on the caller's background executor.

pub mod compose;
pub mod diff;
pub mod file_view;
pub mod fuzzy;
pub mod git;
pub mod handoff;
pub mod highlight;
pub mod host;
pub mod host_state;
pub mod local_socket;
pub mod machine;
pub mod markdown;
pub mod model;
pub mod patch;
pub mod preferences;
pub mod recent;
pub mod reveal;
pub mod review;
pub mod selection;
pub mod session;
pub mod state;
pub mod theme;
pub mod tool_view;
pub mod transcript;
pub mod update;

pub mod history;
pub mod navigation;
