//! Builds an [`AgentRuntime`] from your agents, defaults, and tools.

use super::state::{AgentDefaults, AgentRuntime};
use crate::AgentCatalog;
use reloaded_code_core::permissions::ExpandError;
use reloaded_code_core::{
    default_tools, CustomToolRegistry, SharedToolRegistry, TaskSettings, ToolCatalogEntry,
    ToolFactory,
};

/// Builds an [`AgentRuntime`] step by step.
#[derive(Debug)]
pub struct AgentRuntimeBuilder {
    catalog: AgentCatalog,
    defaults: AgentDefaults,
    task_settings: TaskSettings,
    tools: Vec<ToolCatalogEntry>,
    custom_tool_registry: CustomToolRegistry,
}

impl Default for AgentRuntimeBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRuntimeBuilder {
    /// Creates a builder with empty catalog, empty defaults, default Task settings, and the standard tool set.
    #[inline]
    pub fn new() -> Self {
        Self {
            catalog: AgentCatalog::new(),
            defaults: AgentDefaults::default(),
            task_settings: TaskSettings::default(),
            tools: default_tools(),
            custom_tool_registry: CustomToolRegistry::new(),
        }
    }

    /// Sets the agent catalog.
    #[inline]
    pub fn catalog(mut self, catalog: AgentCatalog) -> Self {
        self.catalog = catalog;
        self
    }

    /// Sets the default settings.
    #[inline]
    pub fn defaults(mut self, defaults: AgentDefaults) -> Self {
        self.defaults = defaults;
        self
    }

    /// Sets the shared Task delegation settings.
    #[inline]
    pub fn task_settings(mut self, task_settings: TaskSettings) -> Self {
        self.task_settings = task_settings;
        self
    }

    /// Sets the maximum number of Task delegation hops.
    #[inline]
    pub fn max_task_depth(mut self, max_depth: u8) -> Self {
        self.task_settings = TaskSettings::with_max_depth(max_depth);
        self
    }

    /// Sets the available tools.
    #[inline]
    pub fn tools(mut self, tools: Vec<ToolCatalogEntry>) -> Self {
        self.tools = tools;
        self
    }

    /// Registers a custom tool factory.
    ///
    /// The factory's name (via [`ToolContext::name`](reloaded_code_core::ToolContext::name))
    /// must match the `name` field of the corresponding [`ToolCatalogEntry`] with kind
    /// [`ToolCatalogKind::Custom`](reloaded_code_core::ToolCatalogKind::Custom).
    pub fn custom_tool(mut self, factory: impl ToolFactory + 'static) -> Self {
        self.custom_tool_registry.insert(factory);
        self
    }

    /// Finishes building and returns the [`AgentRuntime`].
    ///
    /// # Errors
    /// - Returns [`ExpandError`] when any agent's permission configuration contains invalid patterns.
    #[inline]
    pub fn build(self) -> Result<AgentRuntime, ExpandError> {
        AgentRuntime::from_parts(
            self.catalog,
            self.defaults,
            self.task_settings,
            self.tools,
            SharedToolRegistry::from_registry(self.custom_tool_registry),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::AgentRuntimeBuilder;
    use crate::runtime::AgentDefaults;
    use crate::{AgentCatalog, AgentConfig, AgentMode, AgentToolSettings, PermissionRule};
    use indexmap::IndexMap;
    use reloaded_code_core::context::{ToolContext, ToolPrompt};
    use reloaded_code_core::permissions::{ExpandError, PermissionAction};
    use reloaded_code_core::tool_metadata::{glob as glob_meta, read as read_meta};
    use reloaded_code_core::{
        default_tools, CustomTool, CustomToolDefinition, CustomToolFuture, TaskSettings,
        ToolBuildContext, ToolCatalogEntry, ToolCatalogKind, ToolFactory, ToolOutput, ToolResult,
        ToolRunContext,
    };
    use std::sync::Arc;

    type TestResult = Result<(), ExpandError>;

    fn sample_config(name: &str, model: Option<&str>) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode: AgentMode::Subagent,
            description: format!("{name} description").into(),
            model: model.map(Into::into),
            hidden: false,
            temperature: Some(0.3),
            top_p: Some(0.8),
            permission: Default::default(),
            options: Default::default(),
            tool_settings: AgentToolSettings::default(),
            prompt: format!("You are {name}.").into(),
        }
    }

    #[test]
    fn builder_builds_runtime_from_owned_inputs() -> TestResult {
        let catalog = AgentCatalog::from_entries([sample_config("planner", Some("openai/gpt-4o"))]);
        let defaults = AgentDefaults {
            model: Some("openai/gpt-4.1-mini".into()),
            temperature: Some(1.0),
            top_p: Some(0.95),
        };
        let tools = vec![
            ToolCatalogEntry::new(read_meta::NAME, ToolCatalogKind::Read),
            ToolCatalogEntry::new(glob_meta::NAME, ToolCatalogKind::Glob),
        ];

        let runtime = AgentRuntimeBuilder::new()
            .catalog(catalog)
            .defaults(defaults.clone())
            .tools(tools.clone())
            .build()?;

        assert_eq!(
            runtime
                .catalog()
                .by_name("planner")
                .and_then(|config| config.model.as_deref()),
            Some("openai/gpt-4o"),
        );
        assert_eq!(runtime.defaults(), &defaults);
        assert_eq!(runtime.task_settings(), TaskSettings::default());
        assert_eq!(runtime.tools(), tools.as_slice());
        Ok(())
    }

    #[test]
    fn builder_overrides_task_settings() -> TestResult {
        let runtime = AgentRuntimeBuilder::new().max_task_depth(5).build()?;

        assert_eq!(runtime.task_settings(), TaskSettings::with_max_depth(5));
        Ok(())
    }

    #[test]
    fn builder_defaults_to_empty_catalog_defaults_and_default_tools() -> TestResult {
        let runtime = AgentRuntimeBuilder::new().build()?;

        assert_eq!(runtime.catalog().iter().count(), 0);
        assert_eq!(runtime.defaults(), &AgentDefaults::default());
        assert_eq!(runtime.task_settings(), TaskSettings::default());
        assert_eq!(runtime.tools(), default_tools().as_slice());
        Ok(())
    }

    #[test]
    fn builder_caches_permission_rulesets() -> TestResult {
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([AgentConfig {
                name: "planner".into(),
                mode: AgentMode::Subagent,
                description: "planner description".into(),
                model: None,
                hidden: false,
                temperature: None,
                top_p: None,
                permission: IndexMap::from([(
                    read_meta::NAME.into(),
                    PermissionRule::Action(PermissionAction::Allow),
                )]),
                options: Default::default(),
                tool_settings: AgentToolSettings::default(),
                prompt: Default::default(),
            }]))
            .build()?;

        let first = runtime
            .permission_ruleset("planner")
            .expect("cached ruleset should exist");
        let second = runtime
            .permission_ruleset("planner")
            .expect("cached ruleset should exist");

        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.is_allowed(read_meta::NAME, "*"));
        Ok(())
    }

    #[test]
    fn builder_registers_custom_tool() -> TestResult {
        struct TestFactory {
            name: &'static str,
            prompt: &'static str,
        }

        impl TestFactory {
            fn new(name: &'static str, prompt: &'static str) -> Self {
                Self { name, prompt }
            }
        }

        impl ToolContext for TestFactory {
            fn name(&self) -> &'static str {
                self.name
            }

            fn context(&self) -> ToolPrompt {
                ToolPrompt::Static(self.prompt)
            }
        }

        impl ToolFactory for TestFactory {
            fn create(&self, _ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
                Ok(Arc::new(TestTool {
                    name: self.name,
                    prompt: self.prompt,
                }))
            }
        }

        struct TestTool {
            name: &'static str,
            prompt: &'static str,
        }

        impl ToolContext for TestTool {
            fn name(&self) -> &'static str {
                self.name
            }

            fn context(&self) -> ToolPrompt {
                ToolPrompt::Static(self.prompt)
            }
        }

        impl CustomTool for TestTool {
            fn definition(&self) -> CustomToolDefinition {
                CustomToolDefinition::new(self.name, "test tool")
            }

            fn call<'a>(
                &'a self,
                _ctx: ToolRunContext<'a>,
                _args: serde_json::Value,
            ) -> CustomToolFuture<'a> {
                Box::pin(async { Ok(ToolOutput::new("ok")) })
            }
        }

        let runtime = AgentRuntimeBuilder::new()
            .custom_tool(TestFactory::new("stub", "Stub tool guidance."))
            .build()?;

        let factory = runtime.custom_tool_registry().get("stub");
        assert!(
            factory.is_some(),
            "custom tool factory should be registered"
        );
        assert_eq!(factory.unwrap().name(), "stub");
        Ok(())
    }
}
