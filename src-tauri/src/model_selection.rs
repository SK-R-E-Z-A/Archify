//! Model Selection Module
//!
//! This module provides flexible AI model selection with support for:
//! - Global default models
//! - Project-specific model overrides
//! - Responsibility-specific model preferences (planning, coding, review)
//! - Manual model selection
//! - Automatic model selection based on provider capabilities
//! - Safe handling of unavailable/deprecated models

use crate::models::{ProviderConfig, ProviderProfile};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// AI Responsibility Types
// ---------------------------------------------------------------------------

/// Different AI responsibilities that may use different models
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum AiResponsibility {
    /// Planning and reasoning tasks
    Planning,
    /// Prompt generation for coding agents
    PromptGeneration,
    /// Code review and analysis
    CodeReview,
    /// Code execution/agent tasks
    Coding,
    /// General chat/conversation
    Chat,
    /// Default for any unspecified responsibility
    #[default]
    Default,
}

impl AiResponsibility {
    /// Get all responsibility types
    pub fn all() -> Vec<Self> {
        vec![
            Self::Planning,
            Self::PromptGeneration,
            Self::CodeReview,
            Self::Coding,
            Self::Chat,
            Self::Default,
        ]
    }

    /// Display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Planning => "Planning & Reasoning",
            Self::PromptGeneration => "Prompt Generation",
            Self::CodeReview => "Code Review & Analysis",
            Self::Coding => "Coding & Execution",
            Self::Chat => "Chat & Conversation",
            Self::Default => "Default",
        }
    }

    /// Description for UI
    pub fn description(&self) -> &'static str {
        match self {
            Self::Planning => "Models optimized for task planning and strategic reasoning",
            Self::PromptGeneration => "Models optimized for generating coding prompts",
            Self::CodeReview => "Models optimized for code review and quality analysis",
            Self::Coding => "Models optimized for code generation and implementation",
            Self::Chat => "Models optimized for general conversation and assistance",
            Self::Default => "Default model for all responsibilities",
        }
    }
}

// ---------------------------------------------------------------------------
// Model Information
// ---------------------------------------------------------------------------

/// Information about an available AI model
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// Unique model identifier (e.g., "gpt-4o-mini", "claude-3-5-sonnet")
    pub id: String,
    /// Human-readable model name
    pub name: String,
    /// Model description
    #[serde(default)]
    pub description: String,
    /// Provider identifier (e.g., "openai", "anthropic", "ollama")
    pub provider_id: String,
    /// Provider name
    pub provider_name: String,
    /// Model context window size (tokens), if known
    #[serde(default)]
    pub context_window: Option<u64>,
    /// Maximum output tokens, if known
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    /// Whether the model is available/valid
    #[serde(default)]
    pub available: bool,
    /// Whether the model is deprecated
    #[serde(default)]
    pub deprecated: bool,
    /// Capabilities supported by this model
    #[serde(default)]
    pub capabilities: Vec<ModelCapability>,
    /// Model pricing information (for informational purposes only)
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
}

impl ModelInfo {
    /// Create a new model info
    pub fn new(id: String, name: String, provider_id: String, provider_name: String) -> Self {
        Self {
            id,
            name,
            description: String::new(),
            provider_id,
            provider_name,
            context_window: None,
            max_output_tokens: None,
            available: true,
            deprecated: false,
            capabilities: Vec::new(),
            pricing: None,
        }
    }

    /// Check if model supports a specific capability
    pub fn has_capability(&self, capability: ModelCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// Check if model is suitable for a specific responsibility
    pub fn suitable_for(&self, responsibility: AiResponsibility) -> bool {
        if !self.available || self.deprecated {
            return false;
        }

        // Map responsibilities to required capabilities
        let required_capability = match responsibility {
            AiResponsibility::Planning | AiResponsibility::Default => ModelCapability::Planning,
            AiResponsibility::PromptGeneration => ModelCapability::PromptGeneration,
            AiResponsibility::CodeReview => ModelCapability::CodeReview,
            AiResponsibility::Coding => ModelCapability::CodeGeneration,
            AiResponsibility::Chat => ModelCapability::Chat,
        };

        // If model explicitly lists capabilities, check for the required one
        if !self.capabilities.is_empty() {
            return self.has_capability(required_capability)
                || self.has_capability(ModelCapability::All);
        }

        // If no capabilities specified, assume it's a general-purpose model
        true
    }
}

/// Model capabilities
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    /// Can perform planning and reasoning
    Planning,
    /// Can generate prompts
    PromptGeneration,
    /// Can review code
    CodeReview,
    /// Can generate code
    CodeGeneration,
    /// Can handle chat/conversation
    Chat,
    /// Can execute tools/functions
    ToolExecution,
    /// Supports all capabilities
    All,
}

/// Model pricing information (informational only, not used for limits)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ModelPricing {
    /// Input token price (per million tokens)
    #[serde(default)]
    pub input_price_per_million: Option<f64>,
    /// Output token price (per million tokens)
    #[serde(default)]
    pub output_price_per_million: Option<f64>,
    /// Currency (e.g., "USD")
    #[serde(default)]
    pub currency: String,
}

// ---------------------------------------------------------------------------
// Model Preferences
// ---------------------------------------------------------------------------

/// Model preferences for a specific responsibility
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponsibilityModelPreference {
    /// Model ID to use for this responsibility
    #[serde(default)]
    pub model_id: Option<String>,
    /// Whether to use automatic selection if model_id is not set
    #[serde(default)]
    pub auto_select: bool,
}

/// Model selection preferences
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModelPreferences {
    /// Global default model ID
    #[serde(default)]
    pub default_model_id: Option<String>,
    /// Responsibility-specific model preferences
    #[serde(default)]
    pub responsibility_preferences: HashMap<AiResponsibility, ResponsibilityModelPreference>,
    /// Manual overrides (task-specific or session-specific)
    #[serde(default)]
    pub manual_overrides: HashMap<String, String>,
    /// Whether automatic selection is enabled
    #[serde(default = "default_true")]
    pub auto_selection_enabled: bool,
}

impl Default for ModelPreferences {
    fn default() -> Self {
        Self {
            default_model_id: None,
            responsibility_preferences: HashMap::new(),
            manual_overrides: HashMap::new(),
            auto_selection_enabled: true,
        }
    }
}

impl ModelPreferences {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default model
    pub fn set_default_model(&mut self, model_id: Option<String>) {
        self.default_model_id = model_id;
    }

    /// Set model preference for a specific responsibility
    pub fn set_responsibility_preference(
        &mut self,
        responsibility: AiResponsibility,
        preference: ResponsibilityModelPreference,
    ) {
        self.responsibility_preferences
            .insert(responsibility, preference);
    }

    /// Get model preference for a specific responsibility
    pub fn get_responsibility_preference(
        &self,
        responsibility: AiResponsibility,
    ) -> Option<&ResponsibilityModelPreference> {
        self.responsibility_preferences.get(&responsibility)
    }

    /// Set manual override for a specific context (e.g., task ID or session ID)
    pub fn set_manual_override(&mut self, context_id: String, model_id: String) {
        self.manual_overrides.insert(context_id, model_id);
    }

    /// Remove manual override
    pub fn remove_manual_override(&mut self, context_id: &str) {
        self.manual_overrides.remove(context_id);
    }

    /// Get manual override for a specific context
    pub fn get_manual_override(&self, context_id: &str) -> Option<&String> {
        self.manual_overrides.get(context_id)
    }

    /// Enable or disable automatic selection
    pub fn set_auto_selection(&mut self, enabled: bool) {
        self.auto_selection_enabled = enabled;
    }
}

// ---------------------------------------------------------------------------
// Model Selector
// ---------------------------------------------------------------------------

/// Model selector that determines which model to use based on preferences and context
pub struct ModelSelector {
    /// Available models by provider
    available_models: HashMap<String, Vec<ModelInfo>>,
    /// Model preferences
    preferences: ModelPreferences,
    /// Known deprecated models
    deprecated_models: HashSet<String>,
    /// Known invalid/unavailable models
    invalid_models: HashSet<String>,
}

impl ModelSelector {
    pub fn new() -> Self {
        Self {
            available_models: HashMap::new(),
            preferences: ModelPreferences::new(),
            deprecated_models: HashSet::new(),
            invalid_models: HashSet::new(),
        }
    }

    /// Add available models for a provider
    pub fn add_provider_models(&mut self, provider_id: String, models: Vec<ModelInfo>) {
        self.available_models.insert(provider_id, models);
    }

    /// Set deprecated models
    pub fn set_deprecated_models(&mut self, models: HashSet<String>) {
        self.deprecated_models = models;
    }

    /// Set invalid/unavailable models
    pub fn set_invalid_models(&mut self, models: HashSet<String>) {
        self.invalid_models = models;
    }

    /// Set model preferences
    pub fn set_preferences(&mut self, preferences: ModelPreferences) {
        self.preferences = preferences;
    }

    /// Get model preferences
    pub fn preferences(&self) -> &ModelPreferences {
        &self.preferences
    }

    /// Get mutable model preferences
    pub fn preferences_mut(&mut self) -> &mut ModelPreferences {
        &mut self.preferences
    }

    /// Select a model for a given context
    ///
    /// Selection priority:
    /// 1. Manual override (if exists for context)
    /// 2. Responsibility-specific preference
    /// 3. Default model
    /// 4. Automatic selection (if enabled)
    pub fn select_model(
        &self,
        context_id: Option<&str>,
        responsibility: AiResponsibility,
        provider_id: Option<&str>,
    ) -> Option<ModelSelection> {
        // Check for manual override first
        if let Some(context_id) = context_id {
            if let Some(model_id) = self.preferences.get_manual_override(context_id) {
                if let Some(selection) = self.resolve_model_selection(
                    model_id,
                    responsibility,
                    provider_id,
                    SelectionStrategy::ManualOverride,
                ) {
                    return Some(selection);
                }
                // Manual override failed, continue to next option
            }
        }

        // Check responsibility-specific preference
        if let Some(pref) = self
            .preferences
            .get_responsibility_preference(responsibility)
        {
            if let Some(model_id) = &pref.model_id {
                if let Some(selection) = self.resolve_model_selection(
                    model_id,
                    responsibility,
                    provider_id,
                    SelectionStrategy::ResponsibilityPreference,
                ) {
                    return Some(selection);
                }
                // Responsibility preference failed, continue to next option
            }
            if pref.auto_select && self.preferences.auto_selection_enabled {
                return self.auto_select_model(responsibility, provider_id);
            }
        }

        // Check default model
        if let Some(model_id) = &self.preferences.default_model_id {
            if let Some(selection) = self.resolve_model_selection(
                model_id,
                responsibility,
                provider_id,
                SelectionStrategy::DefaultModel,
            ) {
                return Some(selection);
            }
            // Default model failed, continue to next option
        }

        // Try automatic selection
        if self.preferences.auto_selection_enabled {
            return self.auto_select_model(responsibility, provider_id);
        }

        None
    }

    /// Resolve a model selection with validation
    fn resolve_model_selection(
        &self,
        model_id: &str,
        responsibility: AiResponsibility,
        provider_id: Option<&str>,
        strategy: SelectionStrategy,
    ) -> Option<ModelSelection> {
        // Check if model is deprecated or invalid
        if self.deprecated_models.contains(model_id) || self.invalid_models.contains(model_id) {
            return None;
        }

        // Try to find the model in available models
        if let Some(provider_id) = provider_id {
            if let Some(provider_models) = self.available_models.get(provider_id) {
                for model in provider_models {
                    if model.id == model_id && model.suitable_for(responsibility) {
                        return Some(ModelSelection {
                            model_id: model_id.to_string(),
                            model_name: model.name.clone(),
                            provider_id: provider_id.to_string(),
                            provider_name: model.provider_name.clone(),
                            responsibility,
                            strategy,
                            auto_selected: false,
                        });
                    }
                }
            }
        }

        // If we couldn't find it in the specific provider, check all providers
        for (pid, models) in &self.available_models {
            for model in models {
                if model.id == model_id && model.suitable_for(responsibility) {
                    return Some(ModelSelection {
                        model_id: model_id.to_string(),
                        model_name: model.name.clone(),
                        provider_id: pid.clone(),
                        provider_name: model.provider_name.clone(),
                        responsibility,
                        strategy,
                        auto_selected: false,
                    });
                }
            }
        }

        // Model not found in any provider - still return it with unknown provider
        // This allows users to specify models that may not be in our catalog
        if !model_id.is_empty() {
            Some(ModelSelection {
                model_id: model_id.to_string(),
                model_name: model_id.to_string(),
                provider_id: provider_id.unwrap_or("unknown").to_string(),
                provider_name: "Unknown".to_string(),
                responsibility,
                strategy,
                auto_selected: false,
            })
        } else {
            None
        }
    }

    /// Automatic model selection based on capabilities
    pub fn auto_select(
        &self,
        responsibility: AiResponsibility,
        provider_id: Option<&str>,
    ) -> Option<ModelSelection> {
        self.auto_select_model(responsibility, provider_id)
    }

    pub fn auto_select_model(
        &self,
        responsibility: AiResponsibility,
        provider_id: Option<&str>,
    ) -> Option<ModelSelection> {
        let mut candidates: Vec<&ModelInfo> = Vec::new();

        // Get models from specific provider or all providers
        let providers_to_check: Vec<&String> = if let Some(pid) = provider_id {
            self.available_models
                .keys()
                .filter(|k| k.as_str() == pid)
                .collect()
        } else {
            self.available_models.keys().collect()
        };

        for pid in providers_to_check {
            if let Some(models) = self.available_models.get(pid.as_str()) {
                for model in models {
                    if model.suitable_for(responsibility)
                        && !self.deprecated_models.contains(&model.id)
                        && !self.invalid_models.contains(&model.id)
                    {
                        candidates.push(model);
                    }
                }
            }
        }

        // Sort by preference:
        // 1. Models with explicit capability matching the responsibility
        // 2. Models with All capability
        // 3. General-purpose models (no specific capabilities)
        // 4. Alphabetically by ID
        candidates.sort_by(|a, b| {
            let a_has_specific = a
                .capabilities
                .contains(&model_capability_for_responsibility(responsibility));
            let b_has_specific = b
                .capabilities
                .contains(&model_capability_for_responsibility(responsibility));

            match (a_has_specific, b_has_specific) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let a_has_all = a.capabilities.contains(&ModelCapability::All);
                    let b_has_all = b.capabilities.contains(&ModelCapability::All);
                    match (a_has_all, b_has_all) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.id.cmp(&b.id),
                    }
                }
            }
        });

        if let Some(best) = candidates.first() {
            Some(ModelSelection {
                model_id: best.id.clone(),
                model_name: best.name.clone(),
                provider_id: best.provider_id.clone(),
                provider_name: best.provider_name.clone(),
                responsibility,
                strategy: SelectionStrategy::AutoSelected,
                auto_selected: true,
            })
        } else {
            None
        }
    }

    /// Get all available models for a specific provider
    pub fn get_available_models(&self, provider_id: &str) -> Vec<&ModelInfo> {
        self.available_models
            .get(provider_id)
            .map(|models| models.iter().collect())
            .unwrap_or_default()
    }

    /// Get all available models across all providers
    pub fn get_all_available_models(&self) -> Vec<&ModelInfo> {
        self.available_models
            .values()
            .flat_map(|models| models.iter())
            .collect()
    }

    /// Check if a model is available
    pub fn is_model_available(&self, model_id: &str) -> bool {
        if self.deprecated_models.contains(model_id) || self.invalid_models.contains(model_id) {
            return false;
        }

        for models in self.available_models.values() {
            for model in models {
                if model.id == model_id {
                    return model.available && !model.deprecated;
                }
            }
        }
        false
    }

    /// Mark a model as deprecated
    pub fn mark_deprecated(&mut self, model_id: String) {
        self.deprecated_models.insert(model_id);
    }

    /// Mark a model as invalid/unavailable
    pub fn mark_invalid(&mut self, model_id: String) {
        self.invalid_models.insert(model_id);
    }

    /// Clear deprecated and invalid model lists
    pub fn clear_model_status(&mut self) {
        self.deprecated_models.clear();
        self.invalid_models.clear();
    }
}

/// Get the model capability that corresponds to a responsibility
fn model_capability_for_responsibility(responsibility: AiResponsibility) -> ModelCapability {
    match responsibility {
        AiResponsibility::Planning | AiResponsibility::Default => ModelCapability::Planning,
        AiResponsibility::PromptGeneration => ModelCapability::PromptGeneration,
        AiResponsibility::CodeReview => ModelCapability::CodeReview,
        AiResponsibility::Coding => ModelCapability::CodeGeneration,
        AiResponsibility::Chat => ModelCapability::Chat,
    }
}

/// Strategy used for model selection
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// Manual override by user
    ManualOverride,
    /// Responsibility-specific preference
    ResponsibilityPreference,
    /// Global default model
    DefaultModel,
    /// Automatic selection
    AutoSelected,
}

/// Result of model selection
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    /// Selected model ID
    pub model_id: String,
    /// Model name (human-readable)
    pub model_name: String,
    /// Provider ID
    pub provider_id: String,
    /// Provider name
    pub provider_name: String,
    /// Responsibility this model is selected for
    pub responsibility: AiResponsibility,
    /// Strategy used for selection
    pub strategy: SelectionStrategy,
    /// Whether this was auto-selected
    pub auto_selected: bool,
}

impl ModelSelection {
    pub fn new(
        model_id: String,
        model_name: String,
        provider_id: String,
        provider_name: String,
        responsibility: AiResponsibility,
        strategy: SelectionStrategy,
    ) -> Self {
        Self {
            model_id,
            model_name,
            provider_id,
            provider_name,
            responsibility,
            strategy,
            auto_selected: matches!(strategy, SelectionStrategy::AutoSelected),
        }
    }
}

// ---------------------------------------------------------------------------
// Provider Model Information
// ---------------------------------------------------------------------------

/// Information about models available from a specific provider
pub trait ModelProvider {
    /// Get provider ID
    fn provider_id(&self) -> &str;

    /// Get provider name
    fn provider_name(&self) -> &str;

    /// Get available models
    fn available_models(&self) -> Vec<ModelInfo>;
}

/// Built-in model information for known providers
pub struct BuiltInModelProvider {
    id: String,
    name: String,
    models: Vec<ModelInfo>,
}

impl BuiltInModelProvider {
    pub fn new(id: String, name: String, models: Vec<ModelInfo>) -> Self {
        Self { id, name, models }
    }
}

impl ModelProvider for BuiltInModelProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    fn provider_name(&self) -> &str {
        &self.name
    }

    fn available_models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }
}

/// Create built-in model providers for known services
pub fn create_builtin_model_providers() -> Vec<BuiltInModelProvider> {
    vec![
        // Anthropic Claude
        BuiltInModelProvider::new(
            "anthropic".to_string(),
            "Anthropic Claude".to_string(),
            vec![
                ModelInfo::new(
                    "claude-3-5-sonnet-20241022".to_string(),
                    "Claude 3.5 Sonnet".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
                ModelInfo::new(
                    "claude-3-5-sonnet".to_string(),
                    "Claude 3.5 Sonnet (latest)".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
                ModelInfo::new(
                    "claude-3-opus".to_string(),
                    "Claude 3 Opus".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
                ModelInfo::new(
                    "claude-3-sonnet".to_string(),
                    "Claude 3 Sonnet".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
                ModelInfo::new(
                    "claude-3-haiku".to_string(),
                    "Claude 3 Haiku".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
                ModelInfo::new(
                    "claude-2-1".to_string(),
                    "Claude 2.1".to_string(),
                    "anthropic".to_string(),
                    "Anthropic".to_string(),
                ),
            ],
        ),
        // OpenAI
        BuiltInModelProvider::new(
            "openai".to_string(),
            "OpenAI".to_string(),
            vec![
                ModelInfo::new(
                    "gpt-4o-mini".to_string(),
                    "GPT-4o Mini".to_string(),
                    "openai".to_string(),
                    "OpenAI".to_string(),
                ),
                ModelInfo::new(
                    "gpt-4o".to_string(),
                    "GPT-4o".to_string(),
                    "openai".to_string(),
                    "OpenAI".to_string(),
                ),
                ModelInfo::new(
                    "gpt-4-turbo".to_string(),
                    "GPT-4 Turbo".to_string(),
                    "openai".to_string(),
                    "OpenAI".to_string(),
                ),
                ModelInfo::new(
                    "gpt-4".to_string(),
                    "GPT-4".to_string(),
                    "openai".to_string(),
                    "OpenAI".to_string(),
                ),
                ModelInfo::new(
                    "gpt-3.5-turbo".to_string(),
                    "GPT-3.5 Turbo".to_string(),
                    "openai".to_string(),
                    "OpenAI".to_string(),
                ),
            ],
        ),
        // Google Gemini
        BuiltInModelProvider::new(
            "google".to_string(),
            "Google Gemini".to_string(),
            vec![
                ModelInfo::new(
                    "gemini-1.5-flash".to_string(),
                    "Gemini 1.5 Flash".to_string(),
                    "google".to_string(),
                    "Google".to_string(),
                ),
                ModelInfo::new(
                    "gemini-1.5-pro".to_string(),
                    "Gemini 1.5 Pro".to_string(),
                    "google".to_string(),
                    "Google".to_string(),
                ),
                ModelInfo::new(
                    "gemini-1.0-pro".to_string(),
                    "Gemini 1.0 Pro".to_string(),
                    "google".to_string(),
                    "Google".to_string(),
                ),
            ],
        ),
        // Local providers (Ollama, etc.)
        BuiltInModelProvider::new(
            "local".to_string(),
            "Local Models".to_string(),
            vec![
                ModelInfo::new(
                    "qwen2.5-coder".to_string(),
                    "Qwen 2.5 Coder".to_string(),
                    "ollama".to_string(),
                    "Ollama".to_string(),
                ),
                ModelInfo::new(
                    "llama3.1".to_string(),
                    "Llama 3.1".to_string(),
                    "ollama".to_string(),
                    "Ollama".to_string(),
                ),
                ModelInfo::new(
                    "deepseek-coder-v2".to_string(),
                    "DeepSeek Coder v2".to_string(),
                    "ollama".to_string(),
                    "Ollama".to_string(),
                ),
                ModelInfo::new(
                    "codegemma".to_string(),
                    "CodeGemma".to_string(),
                    "ollama".to_string(),
                    "Ollama".to_string(),
                ),
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------
// Helper functions for ProviderConfig and ProviderProfile
// ---------------------------------------------------------------------------

/// Extract model information from a provider profile
pub fn model_info_from_profile(profile: &ProviderProfile) -> Vec<ModelInfo> {
    let mut models = Vec::new();

    // Add cloud model if configured
    if let Some(ref model_id) = profile.config.cloud_model {
        let provider_name = match profile.config.planner_type.as_str() {
            "cloud_http" => {
                if let Some(ref endpoint) = profile.config.cloud_endpoint {
                    // Try to determine provider from endpoint
                    if endpoint.contains("anthropic.com") || endpoint.contains("claude.ai") {
                        "Anthropic"
                    } else if endpoint.contains("openai.com") {
                        "OpenAI"
                    } else if endpoint.contains("google.com")
                        || endpoint.contains("gemini.google.com")
                    {
                        "Google"
                    } else if crate::providers::is_local_endpoint_url(endpoint) {
                        "Local"
                    } else {
                        "Custom"
                    }
                } else {
                    "Custom"
                }
            }
            _ => "Local",
        };

        models.push(ModelInfo::new(
            model_id.clone(),
            model_id.clone(),
            profile.id.clone(),
            provider_name.to_string(),
        ));
    }

    models
}

/// Get the effective model ID from a provider config
pub fn get_effective_model_id(config: &ProviderConfig) -> Option<String> {
    config.cloud_model.clone()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_responsibility_all() {
        let all = AiResponsibility::all();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn test_model_info_new() {
        let model = ModelInfo::new(
            "gpt-4o-mini".to_string(),
            "GPT-4o Mini".to_string(),
            "openai".to_string(),
            "OpenAI".to_string(),
        );
        assert_eq!(model.id, "gpt-4o-mini");
        assert_eq!(model.name, "GPT-4o Mini");
        assert_eq!(model.provider_id, "openai");
        assert_eq!(model.provider_name, "OpenAI");
        assert!(model.available);
        assert!(!model.deprecated);
    }

    #[test]
    fn test_model_info_suitable_for() {
        let mut model = ModelInfo::new(
            "gpt-4o-mini".to_string(),
            "GPT-4o Mini".to_string(),
            "openai".to_string(),
            "OpenAI".to_string(),
        );

        // Without capabilities, model should be suitable for all responsibilities
        assert!(model.suitable_for(AiResponsibility::Planning));
        assert!(model.suitable_for(AiResponsibility::Chat));

        // With specific capability
        model.capabilities = vec![ModelCapability::Planning];
        assert!(model.suitable_for(AiResponsibility::Planning));
        assert!(!model.suitable_for(AiResponsibility::Chat));

        // With All capability
        model.capabilities = vec![ModelCapability::All];
        assert!(model.suitable_for(AiResponsibility::Planning));
        assert!(model.suitable_for(AiResponsibility::Chat));
    }

    #[test]
    fn test_model_preferences_default() {
        let prefs = ModelPreferences::default();
        assert!(prefs.auto_selection_enabled);
        assert!(prefs.default_model_id.is_none());
        assert!(prefs.responsibility_preferences.is_empty());
    }

    #[test]
    fn test_model_selector_basic() {
        let mut selector = ModelSelector::new();

        // Add some models
        let openai_models = vec![
            ModelInfo::new(
                "gpt-4o-mini".to_string(),
                "GPT-4o Mini".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
            ModelInfo::new(
                "gpt-4".to_string(),
                "GPT-4".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
        ];
        selector.add_provider_models("openai".to_string(), openai_models);

        // Test auto-selection
        let selection = selector.select_model(None, AiResponsibility::Chat, Some("openai"));
        assert!(selection.is_some());
        assert_eq!(selection.unwrap().model_id, "gpt-4"); // Alphabetically first
    }

    #[test]
    fn test_model_selector_with_preferences() {
        let mut selector = ModelSelector::new();

        let models = vec![
            ModelInfo::new(
                "gpt-4o-mini".to_string(),
                "GPT-4o Mini".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
            ModelInfo::new(
                "gpt-4".to_string(),
                "GPT-4".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
        ];
        selector.add_provider_models("openai".to_string(), models);

        let mut prefs = ModelPreferences::new();
        prefs.set_default_model(Some("gpt-4o-mini".to_string()));
        selector.set_preferences(prefs);

        // Should use default model
        let selection = selector.select_model(None, AiResponsibility::Chat, Some("openai"));
        assert!(selection.is_some());
        assert_eq!(selection.unwrap().model_id, "gpt-4o-mini");
    }

    #[test]
    fn test_model_selector_deprecated_model() {
        let mut selector = ModelSelector::new();

        let models = vec![
            ModelInfo::new(
                "gpt-4o-mini".to_string(),
                "GPT-4o Mini".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
            ModelInfo::new(
                "gpt-4".to_string(),
                "GPT-4".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
        ];
        selector.add_provider_models("openai".to_string(), models);

        let mut prefs = ModelPreferences::new();
        prefs.set_default_model(Some("gpt-4".to_string()));
        selector.set_preferences(prefs);

        // Mark gpt-4 as deprecated
        selector.mark_deprecated("gpt-4".to_string());

        // Should skip deprecated and select next available
        let selection = selector.select_model(None, AiResponsibility::Chat, Some("openai"));
        assert!(selection.is_some());
        assert_eq!(selection.unwrap().model_id, "gpt-4o-mini");
    }

    #[test]
    fn test_model_selector_manual_override() {
        let mut selector = ModelSelector::new();

        let models = vec![
            ModelInfo::new(
                "gpt-4o-mini".to_string(),
                "GPT-4o Mini".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
            ModelInfo::new(
                "gpt-4".to_string(),
                "GPT-4".to_string(),
                "openai".to_string(),
                "OpenAI".to_string(),
            ),
        ];
        selector.add_provider_models("openai".to_string(), models);

        let mut prefs = ModelPreferences::new();
        prefs.set_default_model(Some("gpt-4".to_string()));
        prefs.set_manual_override("task-123".to_string(), "gpt-4o-mini".to_string());
        selector.set_preferences(prefs);

        // Should use manual override
        let selection =
            selector.select_model(Some("task-123"), AiResponsibility::Chat, Some("openai"));
        assert!(selection.is_some());
        assert_eq!(selection.as_ref().unwrap().model_id, "gpt-4o-mini");
        assert_eq!(
            selection.as_ref().unwrap().strategy,
            SelectionStrategy::ManualOverride
        );
    }

    #[test]
    fn test_builtin_model_providers() {
        let providers = create_builtin_model_providers();
        assert!(providers.len() > 0);

        for provider in &providers {
            assert!(!provider.provider_id().is_empty());
            assert!(!provider.provider_name().is_empty());
            assert!(!provider.available_models().is_empty());
        }
    }

    #[test]
    fn test_model_selector_no_providers() {
        let selector = ModelSelector::new();

        let selection = selector.select_model(None, AiResponsibility::Chat, None);
        assert!(selection.is_none());
    }

    #[test]
    fn test_model_selection_display() {
        let selection = ModelSelection::new(
            "gpt-4o-mini".to_string(),
            "GPT-4o Mini".to_string(),
            "openai".to_string(),
            "OpenAI".to_string(),
            AiResponsibility::Chat,
            SelectionStrategy::AutoSelected,
        );

        assert!(selection.auto_selected);
        assert_eq!(selection.responsibility, AiResponsibility::Chat);
    }
}
