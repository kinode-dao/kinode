use serde::{Deserialize, Serialize};

/// IPC Request format for the timer:distro:sys runtime module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimerAction {
    Debug,
    SetTimer(u64),
}
