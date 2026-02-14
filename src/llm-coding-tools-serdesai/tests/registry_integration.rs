use indexmap::IndexMap;
use llm_coding_tools_agents::{AgentCatalog, AgentConfig, AgentMode, PermissionRule};
use llm_coding_tools_core::permissions::PermissionAction;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuildError, AgentRegistryBuilder, ModelResolveError, ModelResolver,
    ModelsDevResolver, ProviderOverride, ProviderOverrides, ResolutionSource, ResolvedModel,
    TodoState, default_tools,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn catalog_from_json(json: &str) -> ModelsDevCatalog {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let path = temp.path().join("api.json");
    std::fs::write(&path, json).expect("write api.json");
    ModelsDevCatalog::from_local_api_json(&path).expect("catalog")
}

fn base_defaults(resolver: Arc<dyn ModelResolver + Send + Sync>) -> AgentDefaults {
    AgentDefaults {
        model: "openai:gpt-4o".to_string(),
        model_resolver: Some(resolver),
        provider_overrides: ProviderOverrides::new(),
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: HashMap::new(),
    }
}

#[test]
fn registry_builds_mixed_openai_and_openai_compatible() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::set_var("OPENAI_API_KEY", "key");
        std::env::set_var("ROUTER_API_KEY", "key");
    }
    let json = r#"{"providers":{"openai":{"id":"openai","npm":"@ai-sdk/openai","api":null,"env":["OPENAI_API_KEY"],"models":{"gpt-4o":{}}},"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://router.example/v1","env":["ROUTER_API_KEY"],"models":{"m1":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = base_defaults(Arc::new(resolver));

    let config_openai = AgentConfig {
        name: "primary".to_string(),
        mode: AgentMode::Primary,
        description: "primary agent".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let config_router = AgentConfig {
        name: "router".to_string(),
        mode: AgentMode::Subagent,
        description: "router agent".to_string(),
        model: Some("router/m1".to_string()),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let catalog = AgentCatalog::from_entries(vec![config_openai, config_router]);

    let registry = AgentRegistryBuilder::<()>::new(defaults, vec![])
        .build(&catalog)
        .unwrap();
    assert_eq!(registry.iter().count(), 2);

    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ROUTER_API_KEY");
    }
}

#[test]
fn subagents_do_not_inherit_openai_defaults() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("OPENAI_API_KEY", "key") };

    // Ensure ANTHROPIC_API_KEY is not set
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

    let json = r#"{"providers":{"openai":{"id":"openai","npm":"@ai-sdk/openai","api":null,"env":["OPENAI_API_KEY"],"models":{"gpt-4o":{}}},"anthropic":{"id":"anthropic","npm":"@ai-sdk/anthropic","api":null,"env":["ANTHROPIC_API_KEY"],"models":{"claude-3":{}}}}}"#;
    let overrides = ProviderOverrides::new().insert_override(
        "openai",
        ProviderOverride {
            api_key: Some("key".into()),
            base_url: None,
            endpoint_env: None,
        },
    );
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides.clone());
    let defaults = AgentDefaults {
        provider_overrides: overrides,
        ..base_defaults(Arc::new(resolver))
    };

    let config_subagent = AgentConfig {
        name: "anthropic-agent".to_string(),
        mode: AgentMode::Subagent,
        description: "anthropic subagent".to_string(),
        model: Some("anthropic:claude-3".to_string()),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let catalog = AgentCatalog::from_entries(vec![config_subagent]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);

    assert!(matches!(
        result,
        Err(AgentRegistryBuildError::BuildFailed { .. })
    ));

    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[test]
fn unsupported_providers_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let json = r#"{"providers":{"azure":{"id":"azure","npm":"@ai-sdk/azure","api":null,"env":["AZURE_API_KEY"],"models":{"m1":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = base_defaults(Arc::new(resolver));

    let config = AgentConfig {
        name: "azure-agent".to_string(),
        mode: AgentMode::Subagent,
        description: "azure agent".to_string(),
        model: Some("azure:m1".to_string()),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(matches!(
        result,
        Err(AgentRegistryBuildError::BuildFailed { .. })
    ));
}

#[test]
fn registry_builds_huggingface_directly() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("HF_TOKEN", "key") };
    let json = r#"{"providers":{"huggingface":{"id":"huggingface","npm":"@ai-sdk/huggingface","api":null,"env":["HF_TOKEN"],"models":{"tiiuae/falcon-7b":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = base_defaults(Arc::new(resolver));

    let config = AgentConfig {
        name: "hf-agent".to_string(),
        mode: AgentMode::Subagent,
        description: "hf agent".to_string(),
        model: Some("huggingface:tiiuae/falcon-7b".to_string()),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(result.is_ok());

    unsafe {
        std::env::remove_var("HF_TOKEN");
    }
}

#[test]
fn registry_builds_openrouter_directly() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("OPENROUTER_API_KEY", "key") };
    let json = r#"{"providers":{"openrouter":{"id":"openrouter","npm":"@ai-sdk/openrouter","api":null,"env":["OPENROUTER_API_KEY"],"models":{"anthropic/claude-3-opus":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = base_defaults(Arc::new(resolver));

    let config = AgentConfig {
        name: "openrouter-agent".to_string(),
        mode: AgentMode::Subagent,
        description: "openrouter agent".to_string(),
        model: Some("openrouter:anthropic/claude-3-opus".to_string()),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };
    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(result.is_ok());

    unsafe {
        std::env::remove_var("OPENROUTER_API_KEY");
    }
}

#[test]
fn registry_builds_slash_spec_with_colon_model_id() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::set_var("SYNTH_API_KEY", "key");
    }
    let json = r#"{"providers":{"synthetic":{"id":"synthetic","npm":"@ai-sdk/openai-compatible","api":"https://api.synthetic/v1","env":["SYNTH_API_KEY"],"models":{"hf:zai-org/GLM-4.7":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = AgentDefaults {
        model: "synthetic/hf:zai-org/GLM-4.7".to_string(),
        ..base_defaults(Arc::new(resolver))
    };

    let config = AgentConfig {
        name: "synthetic-agent".to_string(),
        mode: AgentMode::Primary,
        description: "synthetic provider".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(result.is_ok());

    unsafe {
        std::env::remove_var("SYNTH_API_KEY");
    }
}

#[test]
fn recursive_builder_injects_task_only_for_allow_configs_and_dedups() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("OPENAI_API_KEY", "key") };

    let json = r#"{"providers":{"openai":{"id":"openai","npm":"@ai-sdk/openai","api":null,"env":["OPENAI_API_KEY"],"models":{"gpt-4o":{}}}}}"#;
    let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
    let defaults = base_defaults(Arc::new(resolver));
    let tools = default_tools(true, None, TodoState::new());

    let mut allow_patterns = IndexMap::new();
    allow_patterns.insert("agent-b".to_string(), PermissionAction::Allow);
    let mut deny_patterns = IndexMap::new();
    deny_patterns.insert("agent-c".to_string(), PermissionAction::Deny);

    let config_a = AgentConfig {
        name: "agent-a".to_string(),
        mode: AgentMode::Subagent,
        description: "a".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::from([(
            tool_names::TASK.to_string(),
            PermissionRule::Pattern(allow_patterns),
        )]),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let config_b = AgentConfig {
        name: "agent-b".to_string(),
        mode: AgentMode::Subagent,
        description: "b".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::from([(
            tool_names::TASK.to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        )]),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let config_c = AgentConfig {
        name: "agent-c".to_string(),
        mode: AgentMode::Subagent,
        description: "c".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::from([(
            tool_names::TASK.to_string(),
            PermissionRule::Pattern(deny_patterns),
        )]),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let catalog = AgentCatalog::from_entries(vec![config_a, config_b, config_c]);
    let registry = AgentRegistryBuilder::<()>::new(defaults, tools)
        .build_with_recursive_task(&catalog, Arc::new(()))
        .unwrap();

    let a = registry.get("agent-a").unwrap();
    let b = registry.get("agent-b").unwrap();
    let c = registry.get("agent-c").unwrap();

    assert_eq!(
        a.tool_names
            .iter()
            .filter(|n| *n == tool_names::TASK)
            .count(),
        1
    );
    assert_eq!(
        b.tool_names
            .iter()
            .filter(|n| *n == tool_names::TASK)
            .count(),
        1
    );
    assert_eq!(
        c.tool_names
            .iter()
            .filter(|n| *n == tool_names::TASK)
            .count(),
        0
    );

    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn registry_builds_with_default_models_dev_resolver_when_none_injected() {
    let provider_overrides = ProviderOverrides::new().insert_override(
        "openai",
        ProviderOverride {
            api_key: Some("test-openai-key".to_string()),
            base_url: None,
            endpoint_env: None,
        },
    );

    let defaults = AgentDefaults {
        model: "openai:gpt-4o".to_string(),
        model_resolver: None,
        provider_overrides,
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: HashMap::new(),
    };

    let config = AgentConfig {
        name: "default-resolver-agent".to_string(),
        mode: AgentMode::Primary,
        description: "default resolver path".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(result.is_ok());
}

// A simple custom resolver for testing injection
#[derive(Debug, Clone)]
struct TestCustomResolver;

impl ModelResolver for TestCustomResolver {
    fn resolve(&self, model_spec: &str) -> Result<ResolvedModel, ModelResolveError> {
        Ok(ResolvedModel {
            spec: model_spec.to_string(),
            runtime_provider: "openai".to_string(),
            runtime_model_id: model_spec.to_string(),
            runtime_spec: format!("openai:{}", model_spec),
            api_key: Some("test-key".to_string()),
            base_url: None,
            timeout: None,
            source: ResolutionSource::ExplicitOverride,
            provider_id: "openai".to_string(),
        })
    }
}

#[test]
fn registry_builds_with_injected_custom_resolver() {
    let custom_resolver = Arc::new(TestCustomResolver);

    let defaults = AgentDefaults {
        model: "custom-model".to_string(),
        model_resolver: Some(custom_resolver),
        provider_overrides: ProviderOverrides::new(),
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: HashMap::new(),
    };

    let config = AgentConfig {
        name: "custom-resolver-agent".to_string(),
        mode: AgentMode::Primary,
        description: "custom resolver test".to_string(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: HashMap::new(),
        prompt: String::new(),
    };

    let catalog = AgentCatalog::from_entries(vec![config]);
    let result = AgentRegistryBuilder::<()>::new(defaults, vec![]).build(&catalog);
    assert!(result.is_ok());
}
