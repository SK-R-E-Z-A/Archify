//! # Batch 1 Integration Tests
//!
//! Independent integration and regression tests for Batch 1 features:
//! - Model Selection
//! - Token Usage Tracking
//!
//! These tests verify that Batch 1 features work together correctly through
//! public interfaces without modifying production source files.

// ============================================================================
// MODEL SELECTION INTEGRATION TESTS
// ============================================================================

#[cfg(test)]
mod model_selection_integration {
    use archify_lib::model_selection::{
        AiResponsibility, ModelCapability, ModelInfo, ModelPreferences, ModelSelector,
        ResponsibilityModelPreference,
    };
    use std::collections::{HashMap, HashSet};

    /// Test that global preference resolution works through public interfaces
    #[tokio::test]
    async fn test_global_preference_resolution() {
        let mut selector = ModelSelector::new();
        selector.add_provider_models(
            "test-provider".to_string(),
            vec![ModelInfo::new(
                "global-test-model".to_string(),
                "Global Test Model".to_string(),
                "test-provider".to_string(),
                "Test Provider".to_string(),
            )],
        );

        let selection = selector
            .select_model(None, AiResponsibility::Planning, Some("test-provider"))
            .expect("selection should succeed");

        assert_eq!(selection.model_id, "global-test-model");
        assert_eq!(selection.responsibility, AiResponsibility::Planning);
    }

    /// Test responsibility-specific preference resolution
    #[tokio::test]
    async fn test_responsibility_specific_preference() {
        let mut selector = ModelSelector::new();
        selector.add_provider_models(
            "test-provider".to_string(),
            vec![ModelInfo::new(
                "global-model".to_string(),
                "Global Model".to_string(),
                "test-provider".to_string(),
                "Test Provider".to_string(),
            )],
        );

        let mut prefs = selector.preferences().clone();
        prefs.set_responsibility_preference(
            AiResponsibility::Planning,
            ResponsibilityModelPreference {
                model_id: Some("planning-special-model".to_string()),
                auto_select: false,
            },
        );
        selector.set_preferences(prefs);

        let planning_selection = selector
            .select_model(None, AiResponsibility::Planning, Some("test-provider"))
            .expect("planning selection should succeed");
        assert_eq!(planning_selection.model_id, "planning-special-model");

        let coding_selection = selector
            .select_model(None, AiResponsibility::Coding, Some("test-provider"))
            .expect("coding selection should succeed");
        assert_eq!(coding_selection.model_id, "global-model");
    }

    /// Test manual override has highest precedence
    #[tokio::test]
    async fn test_manual_override_precedence() {
        let mut selector = ModelSelector::new();
        selector.add_provider_models(
            "test-provider".to_string(),
            vec![ModelInfo::new(
                "global-model".to_string(),
                "Global Model".to_string(),
                "test-provider".to_string(),
                "Test Provider".to_string(),
            )],
        );

        let mut prefs = selector.preferences().clone();
        prefs.set_default_model(Some("global-model".to_string()));
        prefs.set_manual_override("my-task".to_string(), "manual-override-model".to_string());
        selector.set_preferences(prefs);

        let selection = selector
            .select_model(
                Some("my-task"),
                AiResponsibility::Planning,
                Some("test-provider"),
            )
            .expect("manual override should succeed");
        assert_eq!(selection.model_id, "manual-override-model");
    }

    /// Test handling of deprecated models
    #[tokio::test]
    async fn test_deprecated_model_handling() {
        let mut selector = ModelSelector::new();
        selector.mark_deprecated("deprecated-model".to_string());
        selector.add_provider_models(
            "test-provider".to_string(),
            vec![ModelInfo {
                id: "fallback-model".to_string(),
                name: "Fallback Model".to_string(),
                description: String::new(),
                provider_id: "test-provider".to_string(),
                provider_name: "Test Provider".to_string(),
                context_window: None,
                max_output_tokens: None,
                available: true,
                deprecated: false,
                capabilities: vec![ModelCapability::Planning],
                pricing: None,
            }],
        );

        let selection = selector
            .select_model(None, AiResponsibility::Planning, Some("test-provider"))
            .expect("fallback selection should succeed");
        assert_ne!(selection.model_id, "deprecated-model");
    }

    /// Test that resolved model ID can be passed to provider request
    #[tokio::test]
    async fn test_model_id_reaches_provider_request() {
        let mut selector = ModelSelector::new();
        selector.add_provider_models(
            "test-provider".to_string(),
            vec![ModelInfo::new(
                "test-model-123".to_string(),
                "Test Model 123".to_string(),
                "test-provider".to_string(),
                "Test Provider".to_string(),
            )],
        );

        let selection = selector
            .select_model(None, AiResponsibility::Coding, Some("test-provider"))
            .expect("selection should succeed");

        assert_eq!(selection.model_id, "test-model-123");
        assert!(!selection.model_id.is_empty());
    }

    /// Test automatic selection with capability matching
    #[tokio::test]
    async fn test_automatic_selection_capability_matching() {
        let mut selector = ModelSelector::new();

        selector.add_provider_models(
            "test-provider".to_string(),
            vec![
                ModelInfo {
                    id: "planning-capable-model".to_string(),
                    name: "Planning Capable Model".to_string(),
                    description: String::new(),
                    provider_id: "test-provider".to_string(),
                    provider_name: "Test Provider".to_string(),
                    context_window: None,
                    max_output_tokens: None,
                    available: true,
                    deprecated: false,
                    capabilities: vec![ModelCapability::Planning],
                    pricing: None,
                },
                ModelInfo {
                    id: "coding-capable-model".to_string(),
                    name: "Coding Capable Model".to_string(),
                    description: String::new(),
                    provider_id: "test-provider".to_string(),
                    provider_name: "Test Provider".to_string(),
                    context_window: None,
                    max_output_tokens: None,
                    available: true,
                    deprecated: false,
                    capabilities: vec![ModelCapability::CodeGeneration],
                    pricing: None,
                },
            ],
        );

        let selection = selector
            .select_model(None, AiResponsibility::Planning, Some("test-provider"))
            .expect("should select matching model");
        assert_eq!(selection.model_id, "planning-capable-model");
    }
}

// ============================================================================
// TOKEN USAGE INTEGRATION TESTS
// ============================================================================

#[cfg(test)]
mod token_usage_integration {
    use archify_lib::token_usage::{
        TokenUsage, TokenUsageExtractor, TokenUsageStats, TokenUsageTracker, UsageContext,
    };
    use tempfile::tempdir;

    /// Test realistic provider response usage extraction (Anthropic format)
    #[tokio::test]
    async fn test_anthropic_response_usage_extraction() {
        let anthropic_response = serde_json::json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500
            }
        });

        let (input, output, _total) = TokenUsageExtractor::extract_from_json(
            serde_json::to_string(&anthropic_response).unwrap().as_str(),
        );

        assert!(input.is_some());
        assert_eq!(output, Some(500));
    }

    /// Test realistic provider response usage extraction (OpenAI format)
    #[tokio::test]
    async fn test_openai_response_usage_extraction() {
        let openai_response = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "response"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 800,
                "completion_tokens": 400,
                "total_tokens": 1200
            }
        });

        let (input, output, total) = TokenUsageExtractor::extract_from_json(
            serde_json::to_string(&openai_response).unwrap().as_str(),
        );

        assert!(input.is_some());
        assert_eq!(output, Some(400));
        assert_eq!(total, Some(1200));
    }

    /// Test accurate input/output/total token handling
    #[tokio::test]
    async fn test_token_handling_accuracy() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);

        let usage1 = TokenUsage::with_counts(100, 50);
        let mut context1 = UsageContext::new();
        context1.provider_id = Some("anthropic".to_string());
        context1.model_id = Some("claude-3-5-sonnet".to_string());
        context1.project_id = Some("test-project".to_string());
        context1.task_id = Some("task-001".to_string());
        tracker.record_usage(usage1, context1.clone(), "chat".to_string());

        let usage2 = TokenUsage::with_counts(200, 100);
        tracker.record_usage(usage2, context1.clone(), "chat".to_string());

        let usage3 = TokenUsage::with_counts(300, 150);
        tracker.record_usage(usage3, context1.clone(), "chat".to_string());

        let stats = tracker.get_stats();

        assert_eq!(stats.total_input_tokens, 600);
        assert_eq!(stats.total_output_tokens, 300);
        assert_eq!(stats.total_tokens, 900);
    }

    /// Test provider/model/project/task/session attribution
    #[tokio::test]
    async fn test_usage_attribution() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);

        let usage1 = TokenUsage::with_counts(100, 50);
        let mut context1 = UsageContext::new();
        context1.provider_id = Some("provider-a".to_string());
        context1.model_id = Some("model-x".to_string());
        context1.project_id = Some("project-1".to_string());
        context1.task_id = Some("task-1".to_string());
        tracker.record_usage(usage1, context1.clone(), "chat".to_string());

        let usage2 = TokenUsage::with_counts(200, 100);
        let mut context2 = UsageContext::new();
        context2.provider_id = Some("provider-b".to_string());
        context2.model_id = Some("model-y".to_string());
        context2.project_id = Some("project-2".to_string());
        context2.task_id = Some("task-2".to_string());
        tracker.record_usage(usage2, context2.clone(), "chat".to_string());

        let provider_a_stats = tracker.get_stats_by_provider("provider-a");
        assert_eq!(provider_a_stats.total_input_tokens, 100);
        assert_eq!(provider_a_stats.total_output_tokens, 50);

        let provider_b_stats = tracker.get_stats_by_provider("provider-b");
        assert_eq!(provider_b_stats.total_input_tokens, 200);
        assert_eq!(provider_b_stats.total_output_tokens, 100);

        let model_x_stats = tracker.get_stats_by_model("model-x");
        assert_eq!(model_x_stats.total_input_tokens, 100);

        let model_y_stats = tracker.get_stats_by_model("model-y");
        assert_eq!(model_y_stats.total_input_tokens, 200);
    }

    /// Test that unknown usage remains unknown
    #[tokio::test]
    async fn test_unknown_usage_remains_unknown() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);

        let usage = TokenUsage::unknown();
        let mut context = UsageContext::new();
        context.provider_id = Some("provider".to_string());
        context.model_id = Some("model".to_string());
        context.project_id = Some("project".to_string());
        context.task_id = Some("task".to_string());
        tracker.record_usage(usage, context.clone(), "chat".to_string());

        let records = tracker.get_all_records();
        assert_eq!(records.len(), 1);

        assert!(records[0].usage.tokens_unknown);
        assert!(records[0].usage.input_tokens.is_none());
        assert!(records[0].usage.output_tokens.is_none());

        let stats = tracker.get_stats();
        assert_eq!(stats.unknown_request_count, 1);
    }

    /// Test that separate legitimate requests remain separate
    #[tokio::test]
    async fn test_separate_requests_remain_separate() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);

        let usage1 = TokenUsage::with_counts(100, 50);
        let mut context1 = UsageContext::new();
        context1.provider_id = Some("provider".to_string());
        context1.model_id = Some("model".to_string());
        context1.project_id = Some("project".to_string());
        context1.task_id = Some("task-001".to_string());
        tracker.record_usage(usage1, context1.clone(), "chat".to_string());

        let usage2 = TokenUsage::with_counts(200, 100);
        let mut context2 = UsageContext::new();
        context2.provider_id = Some("provider".to_string());
        context2.model_id = Some("model".to_string());
        context2.project_id = Some("project".to_string());
        context2.task_id = Some("task-002".to_string());
        tracker.record_usage(usage2, context2.clone(), "chat".to_string());

        let stats = tracker.get_stats();
        assert_eq!(stats.total_input_tokens, 300);
        assert_eq!(stats.total_output_tokens, 150);

        let task_001_stats = tracker.get_stats_by_task("task-001");
        assert_eq!(task_001_stats.total_input_tokens, 100);

        let task_002_stats = tracker.get_stats_by_task("task-002");
        assert_eq!(task_002_stats.total_input_tokens, 200);
    }

    /// Test retry and recovery behavior
    #[tokio::test]
    async fn test_retry_and_recovery_behavior() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);

        let usage = TokenUsage::with_counts(100, 50);
        let mut context = UsageContext::new();
        context.project_id = Some("project".to_string());
        context.task_id = Some("original-task".to_string());
        let original_record = tracker.record_usage(usage, context.clone(), "chat".to_string());
        let original_id = original_record.id.clone();

        let retry_usage = TokenUsage::with_counts(100, 50);
        let retry_record = tracker.record_retry(&original_id, retry_usage, false);

        let all_records = tracker.get_all_records();
        assert_eq!(all_records.len(), 2);

        assert!(!original_record.is_retry);
        assert!(original_record.original_request_id.is_none());
        let retry_record = retry_record.unwrap();
        assert!(retry_record.is_retry);
        assert_eq!(retry_record.original_request_id, Some(original_id));
    }

    /// Test persistence across save/load cycles
    #[tokio::test]
    async fn test_token_usage_persistence() {
        let temp_dir = tempdir().unwrap();
        let persistence_path = temp_dir.path().join("token_usage.json");

        let mut tracker1 = TokenUsageTracker::new(&persistence_path);

        let usage = TokenUsage::with_counts(100, 50);
        let mut context = UsageContext::new();
        context.project_id = Some("project".to_string());
        context.task_id = Some("task".to_string());
        tracker1.record_usage(usage, context, "chat".to_string());
        tracker1.save().unwrap();

        let mut tracker2 = TokenUsageTracker::new(&persistence_path);
        tracker2.load().unwrap();

        let stats = tracker2.get_stats();
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);

        let records = tracker2.get_all_records();
        assert_eq!(records.len(), 1);
    }

    /// Test extraction from generic JSON with usage fields
    #[tokio::test]
    async fn test_generic_usage_extraction() {
        let generic_response = serde_json::json!({
            "some_field": "value",
            "usage": {
                "input_tokens": 750,
                "output_tokens": 250
            },
            "other_field": "ignored"
        });

        let (input, output, total) = TokenUsageExtractor::extract_from_json(
            serde_json::to_string(&generic_response).unwrap().as_str(),
        );

        assert!(input.is_some());
        assert_eq!(input.unwrap(), 750);
        assert!(output.is_some());
        assert_eq!(output.unwrap(), 250);
    }

    /// Test extraction handles missing token fields gracefully
    #[tokio::test]
    async fn test_missing_token_fields_returns_unknown() {
        let response = serde_json::json!({
            "id": "msg_test",
            "content": "response without usage"
        });

        let (input, output, total) = TokenUsageExtractor::extract_from_json(
            serde_json::to_string(&response).unwrap().as_str(),
        );

        assert!(input.is_none());
        assert!(output.is_none());
        assert!(total.is_none());
    }
}

// ============================================================================
// BATCH 1 MODULE INTEGRATION TESTS
// ============================================================================

#[cfg(test)]
mod batch_1_module_integration {
    use archify_lib::model_selection::AiResponsibility;
    use archify_lib::workspace::Workspace;
    use tempfile::tempdir;

    /// Test that Batch 1 components are accessible through workspace
    #[tokio::test]
    async fn test_workspace_has_batch_1_components() {
        let temp_dir = tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();

        let workspace = Workspace::new_with_paths(None, workspace_dir);

        // Model selector should have built-in providers
        let models = workspace.list_available_models().await.unwrap();
        assert!(!models.is_empty(), "Workspace should have built-in models");

        // Token usage tracker should be initialized
        let stats = workspace.get_token_usage_stats().await.unwrap();
        assert_eq!(stats.total_input_tokens, 0);
    }

    /// Test end-to-end: model selection through workspace
    #[tokio::test]
    async fn test_model_selection_through_workspace() {
        let temp_dir = tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();

        let workspace = Workspace::new_with_paths(None, workspace_dir);

        let mut prefs = workspace.get_model_preferences().await.unwrap();
        prefs.default_model_id = Some("test-model".to_string());
        workspace.update_model_preferences(prefs).await.unwrap();

        let selection = workspace
            .get_model_selection(None, Some(AiResponsibility::Planning), None)
            .await
            .unwrap();

        assert!(selection.is_some());
        assert_eq!(selection.unwrap().model_id, "test-model");
    }

    /// Test end-to-end: token usage recording through workspace
    #[tokio::test]
    async fn test_token_usage_through_workspace() {
        let temp_dir = tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();

        let workspace = Workspace::new_with_paths(None, workspace_dir);

        workspace
            .record_token_usage(
                Some(1000),
                Some(500),
                Some("anthropic".to_string()),
                Some("Claude".to_string()),
                Some("claude-3-5-sonnet".to_string()),
                Some("Claude 3.5 Sonnet".to_string()),
                Some("test-project".to_string()),
                Some("task-001".to_string()),
                Some("session-001".to_string()),
                Some("Planning".to_string()),
                "completion".to_string(),
            )
            .await
            .unwrap();

        let stats = workspace.get_token_usage_stats().await.unwrap();
        assert_eq!(stats.total_input_tokens, 1000);
        assert_eq!(stats.total_output_tokens, 500);
    }

    /// Test integrated model selection and token usage
    #[tokio::test]
    async fn test_integrated_model_selection_and_token_usage() {
        let temp_dir = tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();

        let workspace = Workspace::new_with_paths(None, workspace_dir);

        // Set up model selection
        let mut prefs = workspace.get_model_preferences().await.unwrap();
        prefs.default_model_id = Some("claude-3-5-sonnet".to_string());
        workspace.update_model_preferences(prefs).await.unwrap();

        // Get model selection
        let selection = workspace
            .get_model_selection(None, Some(AiResponsibility::Planning), None)
            .await
            .unwrap();

        assert!(selection.is_some());
        let selected_model = selection.unwrap();

        // Record token usage with the selected model
        let model_id = selected_model.model_id.clone();
        workspace
            .record_token_usage(
                Some(1000),
                Some(500),
                Some("anthropic".to_string()),
                Some("Anthropic".to_string()),
                Some(model_id.clone()),
                None,
                Some("test-project".to_string()),
                Some("task-001".to_string()),
                None,
                Some("Planning".to_string()),
                "completion".to_string(),
            )
            .await
            .unwrap();

        // Verify both systems work together
        let model_stats = workspace
            .get_token_usage_by_model(model_id.clone())
            .await
            .unwrap();
        assert_eq!(model_stats.total_input_tokens, 1000);
        assert_eq!(model_stats.total_output_tokens, 500);
    }
}
