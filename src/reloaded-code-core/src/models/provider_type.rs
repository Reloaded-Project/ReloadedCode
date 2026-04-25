/// Provider behavior profile used by model resolver logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, bitcode::Encode, bitcode::Decode)]
#[repr(u8)]
pub enum ProviderType {
    /// Unknown or unsupported provider package.
    #[default]
    Unknown,
    /// OpenAI chat-completions provider.
    OpenAiCompletions,
    /// OpenAI Responses API provider.
    OpenAiResponses,
    /// Anthropic provider.
    Anthropic,
    /// Google/Gemini provider.
    Google,
    /// Groq provider.
    Groq,
    /// Mistral provider.
    Mistral,
    /// Ollama provider.
    Ollama,
    /// AWS Bedrock provider.
    Bedrock,
    /// Azure-style provider where a base URL is required.
    Azure,
    /// OpenRouter provider.
    OpenRouter,
    /// Hugging Face provider.
    HuggingFace,
    /// Cohere provider.
    Cohere,
    /// ChatGPT OAuth provider.
    ChatGptOAuth,
    /// Claude Code OAuth provider.
    ClaudeCodeOAuth,
    /// Antigravity provider.
    Antigravity,
}

impl ProviderType {
    /// Returns true when this provider requires an API key.
    #[inline]
    pub const fn requires_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    /// Returns true when this provider supports an explicit base URL override.
    #[inline]
    pub const fn supports_base_url(self) -> bool {
        !matches!(self, Self::Bedrock)
    }

    /// Returns true when this provider requires a base URL.
    #[inline]
    pub const fn requires_base_url(self) -> bool {
        matches!(self, Self::Azure)
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderType;
    use rstest::rstest;

    #[test]
    fn unknown_is_default_variant() {
        assert_eq!(ProviderType::default(), ProviderType::Unknown);
    }

    /// Verifies that provider type flags return expected values.
    #[rstest]
    #[case::azure_requires_base_url(ProviderType::Azure, true, true)]
    #[case::ollama_no_api_key(ProviderType::Ollama, false, false)]
    #[case::openai_requires_api_key(ProviderType::OpenAiCompletions, false, true)]
    fn provider_type_flags(
        #[case] provider: ProviderType,
        #[case] requires_base_url: bool,
        #[case] requires_api_key: bool,
    ) {
        assert_eq!(provider.requires_base_url(), requires_base_url);
        assert_eq!(provider.requires_api_key(), requires_api_key);
    }
}
