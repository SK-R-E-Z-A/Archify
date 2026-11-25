//! Centralized constants for the application.

/// Secret API keys that should be redacted from logs and persisted history.
pub const SECRET_ENV_VARS: [&str; 6] = [
    "PLANNER_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "CLAUDE_API_KEY",
];

/// Non-secret provider configuration keys displayed in settings UI.
pub const PROVIDER_CONFIG_KEYS: [&str; 3] = ["PLANNER_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"];

/// Maximum number of iteration attempts for any task (configurable).
pub const DEFAULT_MAX_ITERATIONS: usize = 10;

/// Default timeout for executor processes (30 minutes).
pub const DEFAULT_TIMEOUT_SECS: u64 = 30 * 60;

/// Timeout for verification commands (5 minutes).
pub const VERIFICATION_TIMEOUT_SECS: u64 = 5 * 60;

/// Maximum output size for bounded readers (1 MB).
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Maximum history size for terminal replay (512 KB).
pub const MAX_HISTORY_BYTES: usize = 512 * 1024;

/// Maximum dimension for ConPTY casts (i16 range).
pub const MAX_DIMENSION: u16 = 500;
