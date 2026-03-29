use super::{ResolvedSerdesModel, build_serdes_model};
use crate::agent_runtime::model::resolve_model;
use ahash::AHashMap;
use indexmap::IndexMap;
use llm_coding_tools_agents::{AgentConfig, AgentDefaults, AgentMode, AgentToolSettings};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
    ProviderSource, ProviderType,
};

struct Case {
    provider_key: &'static str,
    provider: ProviderInfo,
    model_name: &'static str,
    credential_updates: &'static [(&'static str, Option<&'static str>)],
    expected_spec: &'static str,
    expected_system: &'static str,
}

fn config_with_model(name: &str, model: Option<&str>) -> AgentConfig {
    AgentConfig {
        name: name.into(),
        mode: AgentMode::All,
        description: Default::default(),
        model: model.map(Into::into),
        hidden: false,
        temperature: None,
        top_p: None,
        permission: IndexMap::new(),
        options: AHashMap::new(),
        tool_settings: AgentToolSettings::default(),
        prompt: Default::default(),
    }
}

fn provider(api_url: &str, env_vars: &[&str], api_type: ProviderType) -> ProviderInfo {
    ProviderInfo {
        api_url: api_url.to_string(),
        env_vars: env_vars
            .iter()
            .map(|env_var| (*env_var).to_string())
            .collect(),
        api_type,
    }
}

fn model_info(max_input: u32, max_output: u32) -> ModelInfo {
    ModelInfo {
        modalities: Modality::TEXT,
        max_input,
        max_output,
        temperature: Some(1.0),
        top_p: Some(0.95),
    }
}

fn build_catalog(
    providers: Vec<(&str, ProviderInfo)>,
    provider_models: Vec<(&str, &str, ModelInfo)>,
) -> ModelCatalog {
    let provider_sources: Vec<ProviderSource> = providers
        .into_iter()
        .map(|(key, info)| ProviderSource::new(key, info))
        .collect();
    let provider_model_sources: Vec<ProviderModelSource<'_>> = provider_models
        .into_iter()
        .map(|(provider_key, model_key, info)| {
            let provider_idx = ProviderIdx::new(
                provider_sources
                    .iter()
                    .position(|provider| provider.provider_key == provider_key)
                    .expect("provider key should exist") as u16,
            );
            ProviderModelSource::new(provider_idx, model_key, info)
        })
        .collect();
    ModelCatalog::build(&provider_sources, &provider_model_sources)
        .expect("catalog fixture should build")
}

fn resolve_case(case: &Case) -> ResolvedSerdesModel {
    let catalog = build_catalog(
        vec![(case.provider_key, case.provider.clone())],
        vec![(
            case.provider_key,
            case.model_name,
            model_info(128_000, 16_384),
        )],
    );
    let model = format!("{}/{}", case.provider_key, case.model_name);
    let defaults = AgentDefaults::with_model(&*model);
    let agent = config_with_model("planner", None);
    let mut credentials = CredentialResolver::without_env();
    for (name, value) in case.credential_updates {
        if let Some(value) = value {
            credentials.set_override(*name, *value);
        }
    }
    let resolved = resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
    build_serdes_model(&catalog, &resolved, &credentials).expect("model should build")
}

#[cfg(feature = "bedrock")]
#[test]
fn build_bedrock_ignores_process_env_when_resolver_disables_env_fallback() {
    let catalog = build_catalog(
        vec![("bedrock", provider("", &[], ProviderType::Bedrock))],
        vec![(
            "bedrock",
            "anthropic.claude-3-5-sonnet-20241022-v2:0",
            model_info(128_000, 16_384),
        )],
    );
    let defaults = AgentDefaults::with_model("bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0");
    let agent = config_with_model("planner", None);
    let credentials = CredentialResolver::without_env();

    temp_env::with_vars(
        [
            ("AWS_ACCESS_KEY_ID", Some("ambient-access-key")),
            ("AWS_SECRET_ACCESS_KEY", Some("ambient-secret-key")),
            ("AWS_REGION", Some("us-east-1")),
        ],
        || {
            let resolved =
                resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
            let err = build_serdes_model(&catalog, &resolved, &credentials)
                .err()
                .expect("model should fail");
            assert!(err.to_string().contains("AWS_ACCESS_KEY_ID"));
        },
    );
}

#[test]
fn build_serdes_model_covers_every_provider_mapping() {
    let mut cases = Vec::with_capacity(15);

    #[cfg(feature = "openai")]
    {
        cases.push(Case {
            provider_key: "synthetic",
            provider: provider(
                "https://api.synthetic.new/v1",
                &["SYNTHETIC_API_KEY"],
                ProviderType::OpenAiCompletions,
            ),
            model_name: "hf:zai-org/GLM-4.7",
            credential_updates: &[("SYNTHETIC_API_KEY", Some("synthetic-key"))],
            expected_spec: "openai:hf:zai-org/GLM-4.7",
            expected_system: "openai",
        });
        cases.push(Case {
            provider_key: "openai",
            provider: provider("", &["OPENAI_API_KEY"], ProviderType::OpenAiResponses),
            model_name: "o3-mini",
            credential_updates: &[("OPENAI_API_KEY", Some("openai-key"))],
            expected_spec: "openai:o3-mini",
            expected_system: "openai",
        });
    }

    #[cfg(feature = "anthropic")]
    cases.push(Case {
        provider_key: "anthropic",
        provider: provider("", &["ANTHROPIC_API_KEY"], ProviderType::Anthropic),
        model_name: "claude-3-5-sonnet-20241022",
        credential_updates: &[("ANTHROPIC_API_KEY", Some("anthropic-key"))],
        expected_spec: "anthropic:claude-3-5-sonnet-20241022",
        expected_system: "anthropic",
    });

    #[cfg(any(feature = "google", feature = "gemini"))]
    cases.push(Case {
        provider_key: "google",
        provider: provider(
            "",
            &["GOOGLE_GENERATIVE_AI_API_KEY", "GEMINI_API_KEY"],
            ProviderType::Google,
        ),
        model_name: "gemini-2.5-flash",
        credential_updates: &[("GOOGLE_GENERATIVE_AI_API_KEY", Some("google-key"))],
        expected_spec: "google:gemini-2.5-flash",
        expected_system: "google",
    });

    #[cfg(feature = "groq")]
    cases.push(Case {
        provider_key: "groq",
        provider: provider(
            serdes_ai_models::GroqModel::BASE_URL,
            &["GROQ_API_KEY"],
            ProviderType::Groq,
        ),
        model_name: "llama-3.3-70b-versatile",
        credential_updates: &[("GROQ_API_KEY", Some("groq-key"))],
        expected_spec: "groq:llama-3.3-70b-versatile",
        expected_system: "groq",
    });

    #[cfg(feature = "mistral")]
    cases.push(Case {
        provider_key: "mistral",
        provider: provider(
            "https://api.mistral.ai/v1",
            &["MISTRAL_API_KEY"],
            ProviderType::Mistral,
        ),
        model_name: "mistral-large-latest",
        credential_updates: &[("MISTRAL_API_KEY", Some("mistral-key"))],
        expected_spec: "mistral:mistral-large-latest",
        expected_system: "mistral",
    });

    #[cfg(feature = "ollama")]
    cases.push(Case {
        provider_key: "ollama",
        provider: provider("http://localhost:11434", &[], ProviderType::Ollama),
        model_name: "llama3.2",
        credential_updates: &[],
        expected_spec: "ollama:llama3.2",
        expected_system: "ollama",
    });

    #[cfg(feature = "bedrock")]
    cases.push(Case {
        provider_key: "bedrock",
        provider: provider("", &[], ProviderType::Bedrock),
        model_name: "anthropic.claude-3-5-sonnet-20241022-v2:0",
        credential_updates: &[
            ("AWS_ACCESS_KEY_ID", Some("test-access-key")),
            ("AWS_SECRET_ACCESS_KEY", Some("test-secret-key")),
            ("AWS_REGION", Some("us-east-1")),
        ],
        expected_spec: "bedrock:anthropic.claude-3-5-sonnet-20241022-v2:0",
        expected_system: "bedrock",
    });

    #[cfg(feature = "azure")]
    cases.push(Case {
        provider_key: "azure",
        provider: provider(
            "",
            &["AZURE_RESOURCE_NAME", "AZURE_API_KEY"],
            ProviderType::Azure,
        ),
        model_name: "gpt-4.1-mini",
        credential_updates: &[
            ("AZURE_RESOURCE_NAME", Some("my-resource")),
            ("AZURE_API_KEY", Some("azure-key")),
        ],
        expected_spec: "azure:gpt-4.1-mini",
        expected_system: "azure",
    });

    #[cfg(feature = "openrouter")]
    cases.push(Case {
        provider_key: "openrouter",
        provider: provider(
            "https://openrouter.ai/api/v1",
            &["OPENROUTER_API_KEY"],
            ProviderType::OpenRouter,
        ),
        model_name: "openai/gpt-4.1-mini",
        credential_updates: &[("OPENROUTER_API_KEY", Some("openrouter-key"))],
        expected_spec: "openrouter:openai/gpt-4.1-mini",
        expected_system: "openrouter",
    });

    #[cfg(feature = "huggingface")]
    cases.push(Case {
        provider_key: "huggingface",
        provider: provider(
            "https://router.huggingface.co/v1",
            &["HF_TOKEN"],
            ProviderType::HuggingFace,
        ),
        model_name: "meta-llama/Llama-3.3-70B-Instruct",
        credential_updates: &[("HF_TOKEN", Some("hf-token"))],
        expected_spec: "huggingface:meta-llama/Llama-3.3-70B-Instruct",
        expected_system: "huggingface",
    });

    #[cfg(feature = "cohere")]
    cases.push(Case {
        provider_key: "cohere",
        provider: provider("", &["COHERE_API_KEY"], ProviderType::Cohere),
        model_name: "command-r-plus",
        credential_updates: &[("COHERE_API_KEY", Some("cohere-key"))],
        expected_spec: "cohere:command-r-plus",
        expected_system: "cohere",
    });

    #[cfg(feature = "chatgpt-oauth")]
    cases.push(Case {
        provider_key: "chatgpt-oauth",
        provider: provider(
            "https://chatgpt.com/backend-api/codex",
            &["CHATGPT_ACCESS_TOKEN", "CHATGPT_ACCOUNT_ID"],
            ProviderType::ChatGptOAuth,
        ),
        model_name: "chatgpt-4o-codex",
        credential_updates: &[
            ("CHATGPT_ACCESS_TOKEN", Some("chatgpt-token")),
            ("CHATGPT_ACCOUNT_ID", Some("acct_123")),
        ],
        expected_spec: "chatgpt-oauth:chatgpt-4o-codex",
        expected_system: "chatgpt-oauth",
    });

    #[cfg(feature = "claude-code-oauth")]
    cases.push(Case {
        provider_key: "claude-code-oauth",
        provider: provider(
            "https://api.anthropic.com",
            &["CLAUDE_CODE_ACCESS_TOKEN"],
            ProviderType::ClaudeCodeOAuth,
        ),
        model_name: "claude-sonnet-4-20250514",
        credential_updates: &[("CLAUDE_CODE_ACCESS_TOKEN", Some("claude-token"))],
        expected_spec: "claude-code-oauth:claude-sonnet-4-20250514",
        expected_system: "claude-code-oauth",
    });

    #[cfg(feature = "antigravity")]
    cases.push(Case {
        provider_key: "antigravity",
        provider: provider(
            "https://cloudcode-pa.googleapis.com",
            &["ANTIGRAVITY_ACCESS_TOKEN", "ANTIGRAVITY_PROJECT_ID"],
            ProviderType::Antigravity,
        ),
        model_name: "gemini-3-flash",
        credential_updates: &[
            ("ANTIGRAVITY_ACCESS_TOKEN", Some("antigravity-token")),
            ("ANTIGRAVITY_PROJECT_ID", Some("project-123")),
        ],
        expected_spec: "antigravity:gemini-3-flash",
        expected_system: "antigravity",
    });

    for case in cases {
        let resolved = resolve_case(&case);
        assert_eq!(resolved.spec.as_ref(), case.expected_spec);
        assert_eq!(resolved.model.system(), case.expected_system);
        assert_eq!(resolved.model.name(), case.model_name);
    }
}

#[test]
fn build_serdes_model_skips_empty_credential_env_vars() {
    let catalog = build_catalog(
        vec![(
            "synthetic",
            provider(
                "https://api.synthetic.new/v1",
                &["PRIMARY_API_KEY", "SECONDARY_API_KEY"],
                ProviderType::OpenAiCompletions,
            ),
        )],
        vec![(
            "synthetic",
            "hf:zai-org/GLM-4.7",
            model_info(128_000, 16_384),
        )],
    );
    let defaults = AgentDefaults::with_model("synthetic/hf:zai-org/GLM-4.7");
    let agent = config_with_model("planner", None);
    let mut credentials = CredentialResolver::without_env();
    credentials.set_override("PRIMARY_API_KEY", "");
    credentials.set_override("SECONDARY_API_KEY", "fallback-key");

    let resolved = resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
    let serdes_model =
        build_serdes_model(&catalog, &resolved, &credentials).expect("model should build");
    assert_eq!(serdes_model.spec.as_ref(), "openai:hf:zai-org/GLM-4.7");
}

#[test]
fn build_serdes_model_returns_clear_error_when_required_credential_missing() {
    let catalog = build_catalog(
        vec![(
            "synthetic",
            provider(
                "https://api.synthetic.new/v1",
                &["SYNTHETIC_API_KEY"],
                ProviderType::OpenAiCompletions,
            ),
        )],
        vec![(
            "synthetic",
            "hf:zai-org/GLM-4.7",
            model_info(128_000, 16_384),
        )],
    );
    let defaults = AgentDefaults::with_model("synthetic/hf:zai-org/GLM-4.7");
    let agent = config_with_model("planner", None);
    let credentials = CredentialResolver::without_env();

    let resolved = resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
    let err = build_serdes_model(&catalog, &resolved, &credentials)
        .err()
        .expect("model should fail");
    assert!(
        err.to_string()
            .contains("provider `synthetic` mapped to serdes `openai` requires a credential")
    );
    assert!(err.to_string().contains("SYNTHETIC_API_KEY"));
}

#[test]
fn build_serdes_model_rejects_unknown_provider_type() {
    let catalog = build_catalog(
        vec![("mystery", provider("", &[], ProviderType::Unknown))],
        vec![("mystery", "m1", model_info(1, 1))],
    );
    let defaults = AgentDefaults::with_model("mystery/m1");
    let agent = config_with_model("planner", None);
    let credentials = CredentialResolver::without_env();

    let resolved = resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
    let err = build_serdes_model(&catalog, &resolved, &credentials)
        .err()
        .expect("model should fail");
    assert!(
        err.to_string()
            .contains("provider `mystery` has no SerdesAI mapping")
    );
}
