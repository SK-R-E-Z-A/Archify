use crate::models::{CliAgentPreset, LocalModelPreset, PromptDelivery};
use crate::providers::resolve_executable;

/// Built-in CLI coding-agent integrations.
///
/// `default_args` and `prompt_delivery` mirror how each tool expects a
/// non-interactive prompt. Users can still override the executable/args.
pub fn cli_agent_presets() -> Vec<CliAgentPreset> {
    let presets = vec![
        (
            "claude",
            "Claude Code",
            "Anthropic Claude Code CLI (`claude -p`, prompt on stdin).",
            "claude",
            Vec::<String>::new(),
            PromptDelivery::PrefixedStdin,
        ),
        (
            "codex",
            "OpenAI Codex CLI",
            "Codex non-interactive mode (`codex exec -`, prompt on stdin).",
            "codex",
            vec!["exec".to_string(), "-".to_string()],
            PromptDelivery::Stdin,
        ),
        (
            "aider",
            "Aider",
            "Aider one-shot message (`aider --message <prompt>`).",
            "aider",
            vec!["--message".to_string()],
            PromptDelivery::Argument,
        ),
        (
            "gemini",
            "Gemini CLI",
            "Google Gemini CLI (`gemini -p`, prompt on stdin).",
            "gemini",
            Vec::<String>::new(),
            PromptDelivery::PrefixedStdin,
        ),
        (
            "opencode",
            "OpenCode",
            "OpenCode non-interactive run (`opencode run <prompt>`).",
            "opencode",
            vec!["run".to_string()],
            PromptDelivery::Argument,
        ),
        (
            "cursor-agent",
            "Cursor Agent CLI",
            "Cursor agent non-interactive mode (`cursor-agent -p`, prompt on stdin).",
            "cursor-agent",
            Vec::<String>::new(),
            PromptDelivery::PrefixedStdin,
        ),
        (
            "custom",
            "Custom CLI",
            "Any executable that accepts a prompt on stdin.",
            "",
            Vec::<String>::new(),
            PromptDelivery::PrefixedStdin,
        ),
    ];

    presets
        .into_iter()
        .map(
            |(id, name, description, executable, args, prompt_delivery)| CliAgentPreset {
                id: id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                default_executable: executable.to_string(),
                default_args: args,
                prompt_delivery,
                available: !executable.is_empty() && resolve_executable(executable).is_some(),
            },
        )
        .collect()
}

/// Built-in OpenAI-compatible local model servers.
pub fn local_model_presets() -> Vec<LocalModelPreset> {
    vec![
        LocalModelPreset {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            description: "Ollama's OpenAI-compatible API (run `ollama serve`).".to_string(),
            endpoint: "http://127.0.0.1:11434/v1".to_string(),
            suggested_models: vec![
                "qwen2.5-coder".to_string(),
                "llama3.1".to_string(),
                "deepseek-coder-v2".to_string(),
            ],
            requires_api_key: false,
        },
        LocalModelPreset {
            id: "lmstudio".to_string(),
            name: "LM Studio".to_string(),
            description: "LM Studio local server (Developer → Local Server).".to_string(),
            endpoint: "http://127.0.0.1:1234/v1".to_string(),
            suggested_models: vec!["local-model".to_string()],
            requires_api_key: false,
        },
        LocalModelPreset {
            id: "llamacpp".to_string(),
            name: "llama.cpp server".to_string(),
            description: "llama.cpp `llama-server` OpenAI-compatible endpoint.".to_string(),
            endpoint: "http://127.0.0.1:8080/v1".to_string(),
            suggested_models: vec!["local-model".to_string()],
            requires_api_key: false,
        },
        LocalModelPreset {
            id: "vllm".to_string(),
            name: "vLLM".to_string(),
            description: "vLLM OpenAI-compatible server.".to_string(),
            endpoint: "http://127.0.0.1:8000/v1".to_string(),
            suggested_models: vec!["local-model".to_string()],
            requires_api_key: false,
        },
        LocalModelPreset {
            id: "text-generation-webui".to_string(),
            name: "Text Generation WebUI".to_string(),
            description: "oobabooga's OpenAI-compatible API extension.".to_string(),
            endpoint: "http://127.0.0.1:5000/v1".to_string(),
            suggested_models: vec!["local-model".to_string()],
            requires_api_key: false,
        },
    ]
}

pub fn find_cli_preset(id: &str) -> Option<CliAgentPreset> {
    cli_agent_presets()
        .into_iter()
        .find(|preset| preset.id == id)
}

pub fn find_local_model_preset(id: &str) -> Option<LocalModelPreset> {
    local_model_presets()
        .into_iter()
        .find(|preset| preset.id == id)
}
