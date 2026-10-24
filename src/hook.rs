//! Agent lifecycle hook support (`PreToolUse` and `PreInvocation`).

architecture_component!(Hook);

mod protocol;
mod runner;
mod virtual_edit;

pub use self::protocol::{HookEventKind, HookProtocolError};
pub use self::runner::execute_hook_json;
