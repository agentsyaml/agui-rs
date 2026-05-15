//! Error re-exports for AG-UI server integrations.

pub use ag_ui_core::{AgUiError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexports_core_error_constructors() {
        let error = AgUiError::other("boom");
        assert_eq!(error.to_string(), "boom");
    }

    #[test]
    fn result_alias_matches_core_result() {
        let result: Result<i32> = Ok(7);
        assert_eq!(result.ok(), Some(7));
    }
}
