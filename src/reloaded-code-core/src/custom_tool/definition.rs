//! Framework-neutral custom tool definitions.

use serde::{Deserialize, Serialize};

/// Model-facing definition for a custom tool.
///
/// This mirrors the common function-calling shape used by LLM frameworks while
/// staying independent from any specific adapter crate. Framework adapters are
/// expected to translate this type to their native tool definition type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomToolDefinition {
    /// Tool name exposed to the model.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema object describing accepted arguments.
    pub parameters_json_schema: serde_json::Value,
    /// Optional strict-schema flag for frameworks/providers that support it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

impl CustomToolDefinition {
    /// Creates a definition with an empty object parameter schema.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters_json_schema: empty_object_schema(),
            strict: None,
        }
    }

    /// Replaces the parameters JSON Schema.
    #[must_use]
    pub fn with_parameters(mut self, schema: impl Into<serde_json::Value>) -> Self {
        self.parameters_json_schema = schema.into();
        self
    }

    /// Sets the optional strict-schema flag.
    #[must_use]
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }
}

fn empty_object_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

#[cfg(test)]
mod tests {
    use super::CustomToolDefinition;
    use serde_json::json;

    #[test]
    fn definition_defaults_to_empty_object_schema() {
        let definition = CustomToolDefinition::new("echo", "Echoes input");

        assert_eq!(definition.name, "echo");
        assert_eq!(definition.description, "Echoes input");
        assert_eq!(definition.parameters_json_schema["type"], "object");
        assert_eq!(definition.parameters_json_schema["properties"], json!({}));
        assert_eq!(
            definition.parameters_json_schema["additionalProperties"],
            false
        );
        assert_eq!(definition.strict, None);
    }

    #[test]
    fn definition_accepts_custom_schema_and_strict_flag() {
        let schema = json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        });

        let definition = CustomToolDefinition::new("echo", "Echoes input")
            .with_parameters(schema.clone())
            .with_strict(true);

        assert_eq!(definition.parameters_json_schema, schema);
        assert_eq!(definition.strict, Some(true));
    }
}
