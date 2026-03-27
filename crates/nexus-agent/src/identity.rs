//! Cross-session identity resolution.
//!
//! Provides [`IdentityResolver`] for discovering related namespaces and sessions
//! that belong to the same canonical agent family or project, enabling
//! cross-namespace recall without merging separate agent silos.

use nexus_storage::repository::NamespaceRepository;
use tracing::debug;

/// Resolves cross-session identity by mapping agent type strings to canonical
/// forms and discovering related namespaces.
pub struct IdentityResolver;

impl IdentityResolver {
    /// Find all namespace IDs whose name matches the canonical form of the
    /// given agent type.
    ///
    /// This catches cases where older sessions stored memories under an alias
    /// (e.g. "claude") while newer sessions use the canonical name ("claude-code").
    pub async fn related_namespace_ids(
        namespace_repo: &NamespaceRepository,
        agent_type: &str,
    ) -> Vec<i64> {
        let canonical = nexus_core::canonicalize_agent_type(agent_type);
        let mut ids = Vec::new();

        if let Ok(all) = namespace_repo.list_all().await {
            for ns in &all {
                let ns_canonical = nexus_core::canonicalize_agent_type(&ns.name);
                if ns_canonical == canonical {
                    ids.push(ns.id);
                }
            }
        }

        debug!(
            canonical_agent = %canonical,
            related_count = ids.len(),
            "Resolved related namespace IDs"
        );
        ids
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_related_namespace_ids_uses_canonical_resolution() {
        // Pure logic test — canonicalize produces consistent keys.
        assert_eq!(
            nexus_core::canonicalize_agent_type("claude"),
            nexus_core::canonicalize_agent_type("claude-code")
        );
        assert_eq!(
            nexus_core::canonicalize_agent_type("pi"),
            nexus_core::canonicalize_agent_type("pi-mono")
        );
        assert_eq!(
            nexus_core::canonicalize_agent_type("omp"),
            nexus_core::canonicalize_agent_type("oh-my-pi")
        );
    }

    #[test]
    fn test_canonicalize_roundtrip_stability() {
        // Canonicalizing an already-canonical string is idempotent.
        let inputs = [
            "claude-code",
            "pi-mono",
            "oh-my-pi",
            "gemini",
            "codex",
            "amp",
            "generic",
        ];
        for input in &inputs {
            let once = nexus_core::canonicalize_agent_type(input);
            let twice = nexus_core::canonicalize_agent_type(&once);
            assert_eq!(once, twice, "canonicalize({}) not idempotent", input);
        }
    }

    #[test]
    fn test_normalize_project_path_variants_resolve_to_same() {
        assert_eq!(
            nexus_core::normalize_project_path("/home/user/project"),
            nexus_core::normalize_project_path("/home/user/project/")
        );
        assert_eq!(
            nexus_core::normalize_project_path("/home/user/project"),
            nexus_core::normalize_project_path("/home/user/project//")
        );
        assert_eq!(
            nexus_core::normalize_project_path("/a/b/c"),
            nexus_core::normalize_project_path("/a//b///c")
        );
    }
}
