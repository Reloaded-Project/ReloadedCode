//! Extension trait for optional ruleset permission checking.
//!
//! Provides a convenient way to check permissions on an optional ruleset,
//! returning `Ok(())` if no ruleset is configured.
//!
//! # Example
//!
//! ```
//! use reloaded_code_core::permissions::{ExpandError, PermissionAction, Rule, Ruleset};
//! use reloaded_code_core::permissions_ext::OptionRulesetExt;
//!
//! # fn main() -> Result<(), ExpandError> {
//! let mut ruleset = Ruleset::new();
//! ruleset.push(Rule::new("*", "*", PermissionAction::Allow)?);
//!
//! // With ruleset configured to allow all
//! let result: Option<&Ruleset> = Some(&ruleset);
//! assert!(result.check("bash", "some-command").is_ok());
//!
//! // Without ruleset (always allows)
//! let no_ruleset: Option<&Ruleset> = None;
//! assert!(no_ruleset.check("bash", "any-command").is_ok()); // Always Ok(())
//! # Ok(())
//! # }
//! ```

use crate::error::{ToolError, ToolResult};
use crate::permissions::{PermissionAction, Ruleset};

/// Extension trait for optional ruleset permission checking.
///
/// Provides a convenient way to check permissions on an optional ruleset,
/// returning `Ok(())` if no ruleset is configured.
pub trait OptionRulesetExt {
    /// Checks if the given subject is allowed, returning an error if denied.
    ///
    /// Returns `Ok(())` if no ruleset is configured or access is allowed.
    fn check(&self, tool_name: &'static str, subject: &str) -> ToolResult<()>;
}

impl OptionRulesetExt for Option<&Ruleset> {
    #[inline(always)]
    fn check(&self, tool_name: &'static str, subject: &str) -> ToolResult<()> {
        match self {
            Some(ruleset) => {
                if ruleset.evaluate(tool_name, subject) == PermissionAction::Deny {
                    Err(ToolError::PermissionDenied {
                        tool: tool_name,
                        subject: subject.to_string(),
                    })
                } else {
                    Ok(())
                }
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{ExpandError, Rule};

    type TestResult = Result<(), ExpandError>;

    #[test]
    fn option_ruleset_ext_without_ruleset_allows_access() {
        let no_ruleset: Option<&Ruleset> = None;
        assert!(no_ruleset.check("read", "/tmp/file.txt").is_ok());
    }

    #[test]
    fn option_ruleset_ext_returns_permission_denied_for_denied_subject() -> TestResult {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new(
            "read",
            "/tmp/allowed.txt",
            PermissionAction::Allow,
        )?);

        let err = Some(&ruleset).check("read", "/tmp/denied.txt").unwrap_err();

        assert!(matches!(
            err,
            ToolError::PermissionDenied {
                tool: "read",
                subject,
            } if subject == "/tmp/denied.txt"
        ));
        Ok(())
    }

    #[test]
    fn option_ruleset_ext_returns_ok_for_allowed() -> TestResult {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("read", "*", PermissionAction::Allow)?);

        assert!(Some(&ruleset).check("read", "/tmp/file.txt").is_ok());
        Ok(())
    }
}
