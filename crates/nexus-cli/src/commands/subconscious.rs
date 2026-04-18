//! Subconscious commands — real-time memory retrieval and transcript streaming.
//!
//! These commands are called by hook integrations (SessionStart, UserPromptSubmit,
//! PreToolUse, Stop) and write XML to stdout for context injection.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use nexus_core::Config;
use nexus_hooks::retrieval::{RetrievalEngine, SubconsciousMode};
use nexus_hooks::sync_state::{soul_content_hash, SyncState};
use nexus_hooks::transcript::{build_ingest_payload, format_for_ingest, read_transcript_from};
use tracing::debug;

#[derive(Subcommand)]
pub enum SubconsciousCommands {
    /// Initialize subconscious for a new session (SessionStart hook)
    SessionStart {
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Retrieve memories relevant to the current prompt (UserPromptSubmit hook)
    Recall {
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Session ID for sync tracking
        #[arg(long)]
        session_id: Option<String>,
        /// Agent type (accepted but currently unused, for hook compatibility)
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },

    /// Check for updates since last sync (PreToolUse hook)
    SyncCheck {
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
        /// Session ID for sync tracking
        #[arg(long)]
        session_id: Option<String>,
        /// Agent type (accepted but currently unused, for hook compatibility)
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },

    /// Stream session transcript to Nexus ingest (Stop hook)
    IngestTranscript {
        /// Path to the JSONL transcript file
        #[arg(long)]
        transcript_path: Option<String>,
        /// Agent type
        #[arg(long, default_value = "claude-code")]
        agent: String,
        /// Session ID
        #[arg(long)]
        session_id: Option<String>,
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Show subconscious pipeline status
    Status {
        /// Working directory
        #[arg(long)]
        cwd: Option<String>,
    },
}

pub async fn execute(command: SubconsciousCommands) -> Result<()> {
    match command {
        SubconsciousCommands::SessionStart { cwd } => execute_session_start(cwd).await,
        SubconsciousCommands::Recall {
            cwd,
            session_id,
            agent: _,
        } => execute_recall(cwd, session_id).await,
        SubconsciousCommands::SyncCheck {
            cwd,
            session_id,
            agent: _,
        } => execute_sync_check(cwd, session_id).await,
        SubconsciousCommands::IngestTranscript {
            transcript_path,
            agent,
            session_id,
            cwd,
        } => execute_ingest_transcript(transcript_path, agent, session_id, cwd).await,
        SubconsciousCommands::Status { cwd } => execute_status(cwd).await,
    }
}

fn resolve_project_root(cwd: Option<&str>) -> PathBuf {
    cwd.map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn load_config() -> Config {
    Config::from_env().unwrap_or_default()
}

async fn execute_session_start(cwd: Option<String>) -> Result<()> {
    let mode = SubconsciousMode::from_env();
    if mode == SubconsciousMode::Off {
        return Ok(());
    }

    let project_root = resolve_project_root(cwd.as_deref());
    let config = load_config();
    let engine = RetrievalEngine::new(&project_root, config);

    // Load hot cache and soul content for initial injection
    let soul_content = engine.load_soul_content();
    // Access internal method via a public wrapper — format_session_start takes &CognitiveCache
    // We need access to the internal load_hot_cache. Since it's private, use the public interface.
    // For session start, we output the initial context via the engine's format_session_start.
    // Read hot cache via canonical load path
    let nexus_dir = project_root.join(".nexus");
    let hot_cache = nexus_agent::cognitive_cache::CognitiveCache::load_or_init(&nexus_dir);

    let output = engine.format_session_start(&hot_cache, soul_content.as_deref());
    if !output.is_empty() {
        println!("{output}");
    }

    Ok(())
}

async fn execute_recall(cwd: Option<String>, session_id: Option<String>) -> Result<()> {
    let mode = SubconsciousMode::from_env();
    if mode == SubconsciousMode::Off {
        return Ok(());
    }

    let project_root = resolve_project_root(cwd.as_deref());
    let config = Config::from_env().unwrap_or_default();
    let engine = RetrievalEngine::new(&project_root, config.clone());

    // Read the prompt from stdin (UserPromptSubmit hook provides the prompt)
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt)?;
    let prompt_text = if prompt.trim().is_empty() {
        "project context".to_string()
    } else {
        extract_prompt_from_stdin(&prompt)
    };

    let sid = session_id.unwrap_or_else(|| "default".to_string());
    let mut sync_state =
        SyncState::load(&project_root, &sid).unwrap_or_else(|_| SyncState::new(&sid));

    // Step 1: Hot cache + soul retrieval (fast, no DB)
    let mut result = engine.retrieve_for_prompt(&prompt_text, &sync_state).await;

    // Step 2: Semantic embedding search (requires DB + embedder)
    if config.embedding.enabled {
        if let Ok(mut storage) =
            nexus_storage::StorageManager::from_url(&config.database_url()).await
        {
            if storage.initialize().await.is_ok() {
                let embedder = nexus_agent::create_embedding_service(&config).await;
                if let Some(svc) = embedder {
                    let ns_repo = nexus_storage::NamespaceRepository::new(storage.pool().clone());
                    let mem_repo = nexus_storage::MemoryRepository::new(storage.pool().clone());

                    if let Ok(Ok(embedding)) = tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        svc.embed(&prompt_text),
                    )
                    .await
                    {
                        let semantic =
                            semantic_search(&mem_repo, &ns_repo, &embedding, &result.recalled)
                                .await;
                        result.recalled = semantic;
                    }
                }
            }
        }
    }

    let output = engine.format_for_stdout(&result);
    if !output.is_empty() {
        println!("{output}");
    }

    // Advance sync state
    let soul_content = result.soul_content.as_deref().unwrap_or("");
    let soul_hash = soul_content_hash(soul_content);
    let hot_cache_count = result.stats.hot_cache_entries;
    sync_state.advance(soul_hash, hot_cache_count, 0);
    if let Err(e) = sync_state.save(&project_root) {
        debug!("Failed to save sync state: {e}");
    }

    Ok(())
}

async fn execute_sync_check(cwd: Option<String>, session_id: Option<String>) -> Result<()> {
    let mode = SubconsciousMode::from_env();
    if mode == SubconsciousMode::Off {
        return Ok(());
    }

    let project_root = resolve_project_root(cwd.as_deref());
    let config = load_config();
    let engine = RetrievalEngine::new(&project_root, config);

    let sid = session_id.unwrap_or_else(|| "default".to_string());
    let sync_state = SyncState::load(&project_root, &sid).unwrap_or_else(|_| SyncState::new(&sid));

    if let Some(output) = engine.check_for_updates(&sync_state) {
        println!("{output}");
    }

    Ok(())
}

async fn execute_ingest_transcript(
    transcript_path: Option<String>,
    agent: String,
    session_id: Option<String>,
    cwd: Option<String>,
) -> Result<()> {
    let transcript_path = match transcript_path {
        Some(p) => PathBuf::from(p),
        None => {
            // Try to read transcript_path from stdin JSON
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            extract_transcript_path(&input)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/dev/null"))
        }
    };

    if !transcript_path.exists() {
        debug!("Transcript file not found: {:?}", transcript_path);
        return Ok(());
    }

    let sid = session_id.unwrap_or_else(|| "default".to_string());
    let cwd_str = cwd.unwrap_or_else(|| ".".to_string());

    // Read sync state for incremental processing
    let project_root = resolve_project_root(Some(&cwd_str));
    let sync_state = SyncState::load(&project_root, &sid).unwrap_or_else(|_| SyncState::new(&sid));
    let start_index = sync_state.last_processed_index.unwrap_or(0);

    let entries =
        read_transcript_from(&transcript_path, start_index).context("Failed to read transcript")?;

    if entries.is_empty() {
        return Ok(());
    }

    let ingest_entries = format_for_ingest(&entries, 1500);
    let payload = build_ingest_payload(&ingest_entries, &agent, &sid, &cwd_str);

    // Write the payload to a temp file and invoke ingest-hook-event via CLI
    let payload_json = serde_json::to_string(&payload)?;
    let new_index = entries.last().map(|e| e.index).unwrap_or(start_index);

    // Clone required values for the closure
    let project_root_clone = project_root.clone();
    let sid_clone = sid.clone();

    // Spawn the nexus ingest-hook-event command as a background process
    let agent_clone = agent.clone();
    let event_name = "stop_transcript".to_string();
    let handle = std::thread::spawn(move || {
        use std::process::{Command, Stdio};
        let mut child = match Command::new("nexus")
            .args([
                "ingest-hook-event",
                "--agent",
                &agent_clone,
                "--event",
                &event_name,
                "--format",
                "auto",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to spawn ingest-hook-event: {e}");
                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(payload_json.as_bytes()) {
                debug!("Failed to write to ingest stdin: {e}");
                return;
            }
        }

        match child.wait() {
            Ok(status) if status.success() => {
                // Advance sync state only on successful ingest
                if let Ok(mut state) = SyncState::load(&project_root_clone, &sid_clone) {
                    let soul_content = {
                        let soul_path = nexus_agent::soul::soul_path();
                        std::fs::read_to_string(&soul_path).ok()
                    };
                    let soul_hash = soul_content_hash(soul_content.as_deref().unwrap_or(""));
                    let nexus_dir = project_root_clone.join(".nexus");
                    let cache =
                        nexus_agent::cognitive_cache::CognitiveCache::load_or_init(&nexus_dir);
                    let hot_cache_count = cache.hot_cache.entries.len();
                    state.advance(soul_hash, hot_cache_count, new_index);
                    if let Err(e) = state.save(&project_root_clone) {
                        debug!("Failed to save sync state after ingest: {e}");
                    }
                }
            }
            Ok(status) => {
                debug!("Ingest command exited with: {status}");
            }
            Err(e) => {
                debug!("Ingest command wait failed: {e}");
            }
        }
    });

    // Wait for ingest to complete before exiting (prevents transcript drop)
    if let Err(e) = handle.join() {
        debug!("Ingest thread panicked: {e:?}");
    }

    Ok(())
}

async fn execute_status(cwd: Option<String>) -> Result<()> {
    let project_root = resolve_project_root(cwd.as_deref());
    let mode = SubconsciousMode::from_env();

    println!("Subconscious Mode: {:?}", mode);

    // Show hot cache stats
    let nexus_dir = project_root.join(".nexus");
    let cache = nexus_agent::cognitive_cache::CognitiveCache::load_or_init(&nexus_dir);
    if cache.hot_cache.entries.is_empty() && cache.cold_index.entries.is_empty() {
        println!("Hot Cache: not initialized");
    } else {
        println!("Hot Cache: {} entries", cache.hot_cache.entries.len());
        println!("Cold Index: {} entries", cache.cold_index.entries.len());
        if let Some(updated) = cache.hot_cache.last_updated {
            println!("Last Updated: {updated}");
        }
    }

    // Show soul.md status
    let soul_path = nexus_agent::soul::soul_path();
    if soul_path.exists() {
        let content = std::fs::read_to_string(&soul_path)?;
        println!("Soul.md: {} bytes", content.len());
    } else {
        println!("Soul.md: not generated");
    }

    // Show session sync states
    let sessions_dir = project_root.join(".nexus").join("sessions");
    if sessions_dir.exists() {
        let mut count = 0;
        for entry in std::fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let sync_path = entry.path().join("sync_state.json");
            if sync_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&sync_path) {
                    if let Ok(state) = serde_json::from_str::<SyncState>(&data) {
                        println!(
                            "Session '{}': last sync {}, index {:?}",
                            state.session_id, state.last_sync_timestamp, state.last_processed_index
                        );
                        count += 1;
                    }
                }
            }
        }
        println!("Active sessions: {count}");
    }

    Ok(())
}

/// Extract the prompt text from hook stdin JSON input.
fn extract_prompt_from_stdin(input: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(input) {
        // Try common hook input shapes
        if let Some(prompt) = val.get("prompt").and_then(|p| p.as_str()) {
            return prompt.to_string();
        }
        if let Some(content) = val.get("content").and_then(|c| c.as_str()) {
            return content.to_string();
        }
        if let Some(message) = val.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    input.trim().to_string()
}

/// Extract transcript_path from hook stdin JSON.
fn extract_transcript_path(input: &str) -> Option<String> {
    let val = serde_json::from_str::<serde_json::Value>(input).ok()?;
    val.get("transcript_path")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
}

/// Perform semantic embedding search and merge with hot cache results.
/// Returns deduplicated entries ranked by relevance.
async fn semantic_search(
    mem_repo: &nexus_storage::MemoryRepository,
    ns_repo: &nexus_storage::NamespaceRepository,
    embedding: &[f32],
    hot_cache_entries: &[nexus_hooks::retrieval::RecalledMemory],
) -> Vec<nexus_hooks::retrieval::RecalledMemory> {
    use nexus_agent::cognitive_cache::ConfidenceTier;
    use nexus_hooks::retrieval::RecalledMemory;
    use nexus_storage::repository::ListMemoryFilters;
    use nexus_vectors::{SearchOptions, SemanticSearch, VectorEntry};

    let mut results = hot_cache_entries.to_vec();
    // We don't have memory_id in RecalledMemory, so track by content hash instead
    let hot_content: std::collections::HashSet<_> = hot_cache_entries
        .iter()
        .map(|r| r.content.clone())
        .collect();

    // Search across all namespaces
    if let Ok(namespaces) = ns_repo.list_all().await {
        for ns in &namespaces {
            let filters = ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: None,
                include_raw: false,
                limit: 50,
                offset: 0,
            };

            if let Ok(memories) = mem_repo.list_filtered(ns.id, filters).await {
                let entries: Vec<VectorEntry> = memories
                    .iter()
                    .filter_map(|m| {
                        m.content_embedding.as_ref().map(|emb| {
                            VectorEntry::new(m.id, emb.clone(), m.category.to_string(), ns.id)
                        })
                    })
                    .collect();

                if entries.is_empty() {
                    continue;
                }

                let search = SemanticSearch::new();
                let options = SearchOptions::with_limit(10).with_threshold(0.65);

                if let Ok((search_results, _)) = search.search(embedding, &entries, &options) {
                    let ids: Vec<i64> = search_results.iter().map(|r| r.id).collect();
                    if let Ok(matched) = mem_repo.get_by_ids(&ids).await {
                        let memory_by_id: std::collections::HashMap<i64, _> =
                            matched.into_iter().map(|m| (m.id, m)).collect();

                        for sr in search_results {
                            if let Some(m) = memory_by_id.get(&sr.id) {
                                // Skip if content already in hot cache
                                if hot_content.contains(&m.content) {
                                    continue;
                                }
                                results.push(RecalledMemory {
                                    content: m.content.clone(),
                                    relevance: sr.score,
                                    tier: ConfidenceTier::from_score(sr.score),
                                    source: format!("semantic:{}", ns.name),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by relevance and take top 5
    results.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(5);
    results
}
