//! Agent lifecycle hook support: agent-neutral tool-call evaluation, plus one adapter per agent.

architecture_component!(Hook);

mod antigravity;
mod event;
mod runner;
mod virtual_edit;
mod warning_queue;

pub use self::antigravity::{AntigravityEvent, AntigravityProtocolError, run_antigravity_hook};
