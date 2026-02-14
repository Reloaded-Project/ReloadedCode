use llm_coding_tools_models_dev::{ModelLimits, ModelsDevCatalog};

#[test]
fn model_limits_type_is_publicly_importable() {
    let _limits = ModelLimits {
        context: 1024,
        output: Some(256),
    };

    // Verify the method exists and is callable with the expected signature
    fn check_signature<'a>(
        catalog: &'a ModelsDevCatalog,
        model_id: &'a str,
    ) -> Option<&'a ModelLimits> {
        catalog.get_model_limits(model_id)
    }
    let _ = check_signature;
}
