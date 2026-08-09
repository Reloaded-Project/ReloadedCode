//! Maps YAML `api_type` string values to [`ProviderType`] enum variants.

use reloaded_code_core::models::ProviderType;

/// Default `api_type` string used when the field is omitted from YAML.
pub const DEFAULT_API_TYPE: &str = "openai-compatible";

/// Maps a YAML `api_type` string to a [`ProviderType`].
///
/// `openai` and `openai-compatible` both map to [`ProviderType::OpenAiCompletions`].
/// `openai` signals actual OpenAI; `openai-compatible` signals any other
/// OpenAI-API-compatible endpoint.
///
/// Returns [`ProviderType::Unknown`] for unrecognized strings.
///
/// # Arguments
///
/// - `s` - the YAML `api_type` string to map.
pub fn api_type_from_str(s: &str) -> ProviderType {
    match s {
        "openai" | "openai-compatible" => ProviderType::OpenAiCompletions,
        "openai-responses" => ProviderType::OpenAiResponses,
        "anthropic" => ProviderType::Anthropic,
        "google" => ProviderType::Google,
        "groq" => ProviderType::Groq,
        "mistral" => ProviderType::Mistral,
        "ollama" => ProviderType::Ollama,
        "bedrock" => ProviderType::Bedrock,
        "azure" => ProviderType::Azure,
        "openrouter" => ProviderType::OpenRouter,
        "huggingface" => ProviderType::HuggingFace,
        "cohere" => ProviderType::Cohere,
        _ => ProviderType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reloaded_code_core::models::ProviderType;
    use rstest::rstest;

    #[rstest]
    #[case::openai("openai", ProviderType::OpenAiCompletions)]
    #[case::openai_compatible("openai-compatible", ProviderType::OpenAiCompletions)]
    #[case::openai_responses("openai-responses", ProviderType::OpenAiResponses)]
    #[case::anthropic("anthropic", ProviderType::Anthropic)]
    #[case::google("google", ProviderType::Google)]
    #[case::groq("groq", ProviderType::Groq)]
    #[case::mistral("mistral", ProviderType::Mistral)]
    #[case::ollama("ollama", ProviderType::Ollama)]
    #[case::bedrock("bedrock", ProviderType::Bedrock)]
    #[case::azure("azure", ProviderType::Azure)]
    #[case::openrouter("openrouter", ProviderType::OpenRouter)]
    #[case::huggingface("huggingface", ProviderType::HuggingFace)]
    #[case::cohere("cohere", ProviderType::Cohere)]
    #[case::unknown("totally-fake-provider", ProviderType::Unknown)]
    #[case::empty("", ProviderType::Unknown)]
    fn api_type_maps_to_correct_provider_type(#[case] input: &str, #[case] expected: ProviderType) {
        assert_eq!(api_type_from_str(input), expected);
    }

    #[test]
    fn default_api_type_is_openai_compatible() {
        assert_eq!(DEFAULT_API_TYPE, "openai-compatible");
        assert_eq!(
            api_type_from_str(DEFAULT_API_TYPE),
            ProviderType::OpenAiCompletions
        );
    }
}
