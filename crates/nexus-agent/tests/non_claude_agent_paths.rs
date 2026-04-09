//! Soak-test slice: non-Claude agent path verification.
//!
//! Exercises the real HookFactory → AgentHook pipeline for every supported
//! non-Claude agent type.  Tests confirm that:
//!
//! 1. The factory routes each agent string to the correct concrete hook type.
//! 2. Each hook reports its honest support tier (WrapperLifecycle vs MonitorOnly).
//! 3. Wrapper-lifecycle agents have CLIHook lifecycle capabilities (atexit session_end).
//! 4. Factory alias resolution works correctly.
//! 5. All hooks produce correct `agent_type()` strings.

use nexus_hooks::{AgentHook, CLIHook, GeminiHook, HookFactory, QwenHook, SupportTier};

// ---------------------------------------------------------------------------
// Behavior 2: Factory routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn factory_creates_cli_hook_for_wrapper_lifecycle_agents() {
    let factory = HookFactory::new();

    // These agents share the CLIHook implementation.
    let wrapper_agents = ["codex", "amp", "opencode", "droid", "hermes"];

    for agent in &wrapper_agents {
        let hook = factory
            .create_hook(agent)
            .expect("{agent} should be supported");
        assert_eq!(
            hook.agent_type(),
            *agent,
            "CLIHook agent_type() should return '{agent}'"
        );
        assert_eq!(
            hook.support_tier(),
            SupportTier::WrapperLifecycle,
            "{agent} should report WrapperLifecycle support tier"
        );
        let caps = hook.lifecycle_capabilities();
        assert!(
            !caps.session_start,
            "{agent} should not support native session_start"
        );
        // CLIHook-based agents support session_end via atexit callback.
        assert!(
            caps.session_end,
            "{agent} should support session_end via atexit"
        );
        assert!(
            !caps.checkpoint,
            "{agent} should not support native checkpoint"
        );
    }
}

#[tokio::test]
async fn factory_creates_dedicated_hooks_for_monitor_only_agents() {
    let factory = HookFactory::new();

    // Gemini and Qwen have dedicated hook types but are monitor-only.
    let monitor_agents = ["gemini", "qwen"];

    for agent in &monitor_agents {
        let hook = factory
            .create_hook(agent)
            .expect("{agent} should be supported");
        assert_eq!(
            hook.agent_type(),
            *agent,
            "{agent} agent_type() should return '{agent}'"
        );
        assert_eq!(
            hook.support_tier(),
            SupportTier::MonitorOnly,
            "{agent} should report MonitorOnly support tier"
        );
    }
}

#[tokio::test]
async fn factory_alias_resolution_routes_to_correct_hook_types() {
    let factory = HookFactory::new();

    // "claude" should resolve to "claude-code" (ClaudeCodeHook).
    let claude_hook = factory
        .create_hook("claude")
        .expect("claude alias should work");
    assert_eq!(
        claude_hook.agent_type(),
        "claude-code",
        "'claude' alias should resolve to claude-code agent"
    );

    // "pimono" should resolve to "pi-mono" (PiMonoHook).
    let pimono_hook = factory
        .create_hook("pimono")
        .expect("pimono alias should work");
    assert_eq!(
        pimono_hook.agent_type(),
        "pi-mono",
        "'pimono' alias should resolve to pi-mono agent"
    );

    // "omp" should resolve to "oh-my-pi" (OhMyPiHook).
    let omp_hook = factory.create_hook("omp").expect("omp alias should work");
    assert_eq!(
        omp_hook.agent_type(),
        "oh-my-pi",
        "'omp' alias should resolve to oh-my-pi agent"
    );

    // "ohmypi" should also resolve to "oh-my-pi".
    let ohmypi_hook = factory
        .create_hook("ohmypi")
        .expect("ohmypi alias should work");
    assert_eq!(
        ohmypi_hook.agent_type(),
        "oh-my-pi",
        "'ohmypi' alias should resolve to oh-my-pi agent"
    );
}

#[tokio::test]
async fn factory_rejects_unknown_agent_type() {
    let factory = HookFactory::new();
    let result = factory.create_hook("nonexistent-agent-xyz");
    assert!(result.is_err(), "Factory should reject unknown agent types");
}

#[tokio::test]
async fn all_supported_agents_are_actually_supported() {
    let factory = HookFactory::new();

    let known_agents = [
        "claude-code",
        "gemini",
        "qwen",
        "pi-mono",
        "oh-my-pi",
        "pi-skills",
        "opencode",
        "codex",
        "amp",
        "droid",
        "hermes",
        "generic",
    ];

    for agent in &known_agents {
        assert!(
            factory.is_supported(agent),
            "{agent} should be in the supported list"
        );
    }
}

// ---------------------------------------------------------------------------
// Behavior 2: Direct hook instantiation exercises real paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cli_hook_codex_detect_activity_returns_codex_type() {
    let hook = CLIHook::new("codex");
    let activity = hook.detect_session_activity().await.unwrap();
    // The agent_type field in SessionActivity should match.
    assert_eq!(
        format!("{}", activity.agent_type),
        "codex",
        "SessionActivity agent_type should be codex"
    );
}

#[tokio::test]
async fn cli_hook_hermes_detect_activity_returns_hermes_type() {
    let hook = CLIHook::new("hermes");
    let activity = hook.detect_session_activity().await.unwrap();
    assert_eq!(
        format!("{}", activity.agent_type),
        "hermes",
        "SessionActivity agent_type should be hermes"
    );
}

#[tokio::test]
async fn gemini_hook_reports_monitor_only_capabilities() {
    let hook = GeminiHook::new();
    assert_eq!(
        hook.agent_type(),
        "gemini",
        "GeminiHook agent_type() should be 'gemini'"
    );
    assert_eq!(
        hook.support_tier(),
        SupportTier::MonitorOnly,
        "GeminiHook should report MonitorOnly"
    );
}

#[tokio::test]
async fn qwen_hook_reports_monitor_only_capabilities() {
    let hook = QwenHook::new();
    assert_eq!(
        hook.agent_type(),
        "qwen",
        "QwenHook agent_type() should be 'qwen'"
    );
    assert_eq!(
        hook.support_tier(),
        SupportTier::MonitorOnly,
        "QwenHook should report MonitorOnly"
    );
}
