//! Model evaluation suite for Nexus Memory System
//!
//! Tests a model against all aspects of the memory system and produces a scored report.
//! This is a developer tool that evaluates LLM quality for memory operations.

use std::path::PathBuf;
use std::time::Instant;

use nexus_core::config::{AgentConfig, Config, LlmConfig};
use nexus_llm::{create_client_with_fallback, ChatMessage, GenerateParams, LlmClient};
use nexus_storage::repository::{MemoryRelationRepository, MemoryRepository, NamespaceRepository};
use nexus_storage::StorageManager;

use crate::commands::llm::build_test_config;

/// Evaluation result for a single test
#[derive(Debug, Clone)]
struct EvalResult {
    name: String,
    passed: bool,
    score: u32,
    max_score: u32,
    details: String,
}

/// Overall evaluation report
struct EvalReport {
    provider: String,
    model: String,
    total_score: u32,
    max_total: u32,
    results: Vec<EvalResult>,
    elapsed_secs: f64,
}

impl EvalReport {
    fn print(&self) {
        println!();
        println!("{:=<55}", "");
        println!("  Nexus Model Evaluation Report");
        println!("{:=<55}", "");
        println!();
        println!("  Provider: {}", self.provider);
        println!("  Model:    {}", self.model);
        println!("  Time:     {:.1}s", self.elapsed_secs);
        println!();
        println!("  Results:");
        println!("  {:-<53}", "");

        for (i, result) in self.results.iter().enumerate() {
            let status = if result.passed { "PASS" } else { "FAIL" };
            println!(
                "  {}. {:<30} {:>3}/{:<3}  {}",
                i + 1,
                result.name,
                result.score,
                result.max_score,
                status,
            );
            // Print details indented, word-wrapping at ~45 chars per line
            let details = &result.details;
            let mut line = String::from("     ");
            for word in details.split_whitespace() {
                if line.len() + word.len() + 1 > 52 {
                    println!("{}", line);
                    line = String::from("     ");
                }
                if line.len() > 5 {
                    line.push(' ');
                }
                line.push_str(word);
            }
            if !line.trim().is_empty() {
                println!("{}", line);
            }
            println!();
        }

        println!("  {:-<53}", "");
        let pct = if self.max_total > 0 {
            (self.total_score as f64 / self.max_total as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "  Total: {}/{} ({:.1}%)",
            self.total_score, self.max_total, pct
        );

        let verdict = if pct >= 80.0 {
            "GOOD -- model is suitable for memory system operations"
        } else if pct >= 60.0 {
            "ACCEPTABLE -- model works but may miss nuances"
        } else if pct >= 40.0 {
            "WEAK -- model struggles with memory system requirements"
        } else {
            "POOR -- model is not recommended for memory system operations"
        };
        println!("  Verdict: {}", verdict);
        println!();
    }
}

/// Main entry point for the eval command
pub async fn execute(
    provider_override: Option<String>,
    model_override: Option<String>,
) -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let llm_config = build_test_config(&config, provider_override, model_override);

    // Verify API key is set
    match std::env::var(&llm_config.api_key_env) {
        Ok(_) => {}
        Err(_) => {
            anyhow::bail!(
                "Environment variable {} is not set. Set it or run: nexus config set {} YOUR_API_KEY",
                llm_config.api_key_env,
                llm_config.api_key_env
            );
        }
    }

    println!("Initializing evaluation environment...");

    // Create a temporary database for evaluation isolation
    let db_path = PathBuf::from(format!("/tmp/nexus-eval-{}.db", uuid::Uuid::new_v4()));
    let db_url = format!("sqlite:{}", db_path.display());

    let mut storage = StorageManager::from_url(&db_url).await?;
    storage.initialize().await?;

    // Ensure cleanup even on error
    let cleanup_db = |path: &std::path::Path| {
        let _ = std::fs::remove_file(path);
    };

    let result = run_evaluation(&llm_config, &storage).await;

    // Always clean up the temp database
    cleanup_db(&db_path);

    result
}

/// Run all evaluation tests
async fn run_evaluation(llm_config: &LlmConfig, storage: &StorageManager) -> anyhow::Result<()> {
    let pool = storage.pool().clone();
    let namespace_repo = NamespaceRepository::new(pool.clone());
    let memory_repo = MemoryRepository::new(pool.clone());

    // Create evaluation namespace
    let namespace = namespace_repo
        .get_or_create("nexus-eval", "eval")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create eval namespace: {}", e))?;
    let namespace_id = namespace.id;

    // Create LLM client
    let client = create_client_with_fallback(llm_config)
        .map_err(|e| anyhow::anyhow!("Failed to create LLM client: {}", e))?;

    let agent_config = AgentConfig::default();

    let provider = client.provider_name();
    let model = client.model_name();

    println!("  Provider: {}", provider);
    println!("  Model:    {}", model);
    println!();
    println!("Running 6 evaluation tests...");
    println!();

    let start = Instant::now();
    let mut results = Vec::new();

    // Test 1: Instruction Following
    println!("  [1/6] Instruction Following...");
    results.push(test_instruction_following(&*client).await);

    // Test 2: Memory Extraction Quality
    println!("  [2/6] Memory Extraction Quality...");
    let ingest = nexus_agent::IngestService::new(client.clone(), agent_config.clone());
    results.push(test_extraction_quality(&ingest, &memory_repo, namespace_id).await);

    // Test 3: Detail & Nuance Capture
    println!("  [3/6] Detail & Nuance Capture...");
    results.push(test_detail_capture(&ingest, &memory_repo, namespace_id).await);

    // Test 4: Memory Structure
    println!("  [4/6] Memory Structure...");
    results.push(test_memory_structure(&ingest, &memory_repo, namespace_id).await);

    // Test 5: Query / Whisper Quality
    println!("  [5/6] Query / Whisper Quality...");
    let relation_repo = MemoryRelationRepository::new(storage.pool());
    let query_svc = nexus_agent::QueryService::new(client.clone(), agent_config.clone());
    results.push(
        test_query_quality(
            &query_svc,
            &ingest,
            &memory_repo,
            &relation_repo,
            namespace_id,
        )
        .await,
    );

    // Test 6: Consolidation / Dream Quality
    println!("  [6/6] Consolidation / Dream Quality...");
    let consolidate_svc = nexus_agent::ConsolidateService::new(client.clone(), agent_config);
    let relation_repo = MemoryRelationRepository::new(storage.pool());
    results.push(
        test_consolidation_quality(&consolidate_svc, &memory_repo, &relation_repo, namespace_id)
            .await,
    );

    let elapsed = start.elapsed();

    let total_score: u32 = results.iter().map(|r| r.score).sum();
    let max_total: u32 = results.iter().map(|r| r.max_score).sum();

    let report = EvalReport {
        provider,
        model,
        total_score,
        max_total,
        results,
        elapsed_secs: elapsed.as_secs_f64(),
    };

    report.print();

    Ok(())
}

// ---------------------------------------------------------------------------
// Test 1: Instruction Following
// ---------------------------------------------------------------------------

async fn test_instruction_following(client: &dyn LlmClient) -> EvalResult {
    let params = GenerateParams {
        messages: vec![
            ChatMessage::system(
                "You must respond with valid JSON only. No explanation, no markdown, \
                 no extra text. Return exactly this JSON object with these exact values: \
                 {\"color\": \"blue\", \"number\": 42}",
            ),
            ChatMessage::user("What is 2+2?"),
        ],
        max_tokens: 100,
        temperature: 0.0,
        json_mode: true,
    };

    let response = client.generate(params).await;
    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    let raw = match response {
        Ok(resp) => resp.content.trim().to_string(),
        Err(e) => {
            return EvalResult {
                name: "Instruction Following".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("LLM call failed: {}", e),
            };
        }
    };

    // Strip markdown fences if present
    let json_str = if raw.starts_with("```") {
        let start = raw.find('\n').unwrap_or(3) + 1;
        let end = raw.rfind("```").unwrap_or(raw.len());
        raw[start..end].trim().to_string()
    } else {
        raw.clone()
    };

    // Check valid JSON (30 points)
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json_str);
    if parsed.is_ok() {
        score += 30;
        notes.push("Valid JSON produced.".to_string());
    } else {
        notes.push("Failed to produce valid JSON.".to_string());
    }

    if let Ok(obj) = parsed {
        // Check "color" key exists (15 points)
        if obj.get("color").is_some() {
            score += 15;
            // Check "color" value is "blue" (15 points)
            if obj["color"].as_str() == Some("blue") {
                score += 15;
                notes.push("\"color\" key correct.".to_string());
            } else {
                let val = &obj["color"];
                notes.push(format!(
                    "\"color\" key present but value is {} (expected \"blue\").",
                    val
                ));
            }
        } else {
            notes.push("\"color\" key missing.".to_string());
        }

        // Check "number" key exists (15 points)
        if obj.get("number").is_some() {
            score += 15;
            // Check "number" value is 42 (25 points)
            if obj["number"].as_i64() == Some(42) || obj["number"].as_f64() == Some(42.0) {
                score += 10;
                notes.push("\"number\" key correct.".to_string());
            } else {
                let val = &obj["number"];
                notes.push(format!(
                    "\"number\" key present but value is {} (expected 42).",
                    val
                ));
            }
        } else {
            notes.push("\"number\" key missing.".to_string());
        }
    }

    let passed = score >= 50;
    EvalResult {
        name: "Instruction Following".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Test 2: Memory Extraction Quality
// ---------------------------------------------------------------------------

async fn test_extraction_quality(
    ingest: &nexus_agent::IngestService,
    memory_repo: &MemoryRepository,
    namespace_id: i64,
) -> EvalResult {
    let text = "The Nexus Memory System uses SQLite for persistence and supports \
                8 LLM providers including OpenAI, Anthropic, and Google Gemini. \
                It was designed by scooter-lacroix and released under version 1.1.2.";

    let memory_id = match ingest
        .ingest(text, "eval-test-2", namespace_id, memory_repo)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return EvalResult {
                name: "Extraction Quality".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Ingest failed: {}", e),
            };
        }
    };

    // Retrieve the stored memory
    let memory = match memory_repo.get_by_id(memory_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return EvalResult {
                name: "Extraction Quality".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: "Memory stored but could not be retrieved.".to_string(),
            };
        }
        Err(e) => {
            return EvalResult {
                name: "Extraction Quality".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Failed to retrieve memory: {}", e),
            };
        }
    };

    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    let agent_meta = memory.metadata.get("agent");

    // Summary non-empty (25 points)
    let summary = agent_meta
        .and_then(|a| a.get("summary"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if !summary.is_empty() {
        score += 25;
        notes.push("Summary extracted.".to_string());
    } else {
        notes.push("Summary missing or empty.".to_string());
    }

    // Entities non-empty (25 points)
    let entities: Vec<&str> = agent_meta
        .and_then(|a| a.get("entities"))
        .and_then(|e| e.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !entities.is_empty() {
        score += 25;
        notes.push(format!("{} entities extracted.", entities.len()));
    } else {
        notes.push("No entities extracted.".to_string());
    }

    // Topics non-empty (25 points)
    let topics: Vec<&str> = agent_meta
        .and_then(|a| a.get("topics"))
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !topics.is_empty() {
        score += 25;
        notes.push(format!("{} topics assigned.", topics.len()));
    } else {
        notes.push("No topics assigned.".to_string());
    }

    // Importance score in valid range [0, 1] (25 points)
    let importance = agent_meta
        .and_then(|a| a.get("importance_score"))
        .and_then(|s| s.as_f64());
    match importance {
        Some(val) if (0.0..=1.0).contains(&val) => {
            score += 25;
            notes.push(format!("Importance score: {:.2}.", val));
        }
        Some(val) => {
            notes.push(format!(
                "Importance score {} out of valid range [0, 1].",
                val
            ));
        }
        None => {
            notes.push("Importance score missing.".to_string());
        }
    }

    let passed = score >= 50;
    EvalResult {
        name: "Extraction Quality".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Test 3: Detail & Nuance Capture
// ---------------------------------------------------------------------------

async fn test_detail_capture(
    ingest: &nexus_agent::IngestService,
    memory_repo: &MemoryRepository,
    namespace_id: i64,
) -> EvalResult {
    let text = "Although the project uses Rust edition 2021, it requires MSRV 1.75 \
                due to async-trait usage. The developer prefers clippy warnings to be \
                treated as errors (zero-tolerance policy), which was established after \
                repeated issues with silent quality degradation in the hooks crate.";

    let memory_id = match ingest
        .ingest(text, "eval-test-3", namespace_id, memory_repo)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return EvalResult {
                name: "Detail & Nuance Capture".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Ingest failed: {}", e),
            };
        }
    };

    let memory = match memory_repo.get_by_id(memory_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return EvalResult {
                name: "Detail & Nuance Capture".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: "Memory stored but could not be retrieved.".to_string(),
            };
        }
        Err(e) => {
            return EvalResult {
                name: "Detail & Nuance Capture".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Failed to retrieve memory: {}", e),
            };
        }
    };

    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    // Check both the summary and the raw content for keyword capture
    let summary = memory
        .metadata
        .get("agent")
        .and_then(|a| a.get("summary"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_lowercase();

    let entities_str = memory
        .metadata
        .get("agent")
        .and_then(|a| a.get("entities"))
        .and_then(|e| serde_json::to_string(e).ok())
        .unwrap_or_default()
        .to_lowercase();

    let combined = format!("{} {}", summary, entities_str);

    // Mentions MSRV or 1.75 (25 points)
    if combined.contains("msrv") || combined.contains("1.75") {
        score += 25;
        notes.push("MSRV/1.75 captured.".to_string());
    } else {
        notes.push("MSRV/1.75 not captured.".to_string());
    }

    // Mentions edition 2021 (20 points)
    if combined.contains("2021") || combined.contains("edition") {
        score += 20;
        notes.push("Edition 2021 captured.".to_string());
    } else {
        notes.push("Edition 2021 not captured.".to_string());
    }

    // Mentions clippy/error policy (25 points)
    if combined.contains("clippy")
        || combined.contains("zero-tolerance")
        || combined.contains("warning")
        || combined.contains("error")
    {
        score += 25;
        notes.push("Clippy/error policy captured.".to_string());
    } else {
        notes.push("Clippy/error policy not captured.".to_string());
    }

    // Context preserved -- hooks crate mentioned (15 points)
    if combined.contains("hook") || combined.contains("quality") || combined.contains("degradation")
    {
        score += 15;
        notes.push("Context about hooks/quality preserved.".to_string());
    } else {
        notes.push("Context about hooks/quality not preserved.".to_string());
    }

    // Summary is substantive (15 points)
    if summary.len() > 20 {
        score += 15;
        notes.push(format!("Summary is {} chars.", summary.len()));
    } else {
        notes.push(format!("Summary too short ({} chars).", summary.len()));
    }

    let passed = score >= 50;
    EvalResult {
        name: "Detail & Nuance Capture".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Test 4: Memory Structure
// ---------------------------------------------------------------------------

async fn test_memory_structure(
    ingest: &nexus_agent::IngestService,
    memory_repo: &MemoryRepository,
    namespace_id: i64,
) -> EvalResult {
    let text = "Evaluation test for memory structure verification. The system should \
                store this with proper category, labels, and metadata fields.";

    let memory_id = match ingest
        .ingest(text, "eval-test-4", namespace_id, memory_repo)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return EvalResult {
                name: "Memory Structure".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Ingest failed: {}", e),
            };
        }
    };

    let memory = match memory_repo.get_by_id(memory_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return EvalResult {
                name: "Memory Structure".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: "Memory stored but could not be retrieved.".to_string(),
            };
        }
        Err(e) => {
            return EvalResult {
                name: "Memory Structure".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Failed to retrieve memory: {}", e),
            };
        }
    };

    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    // Category is valid (one of 6 known values) (25 points)
    let category_str = memory.category.to_string();
    let valid_categories = [
        "general",
        "facts",
        "preferences",
        "context",
        "specifications",
        "session",
    ];
    if valid_categories.contains(&category_str.as_str()) {
        score += 25;
        notes.push(format!("Category: {}.", category_str));
    } else {
        notes.push(format!("Invalid category: {}.", category_str));
    }

    // Labels non-empty (25 points)
    if !memory.labels.is_empty() {
        score += 25;
        notes.push(format!("{} labels present.", memory.labels.len()));
    } else {
        notes.push("No labels present.".to_string());
    }

    // Metadata has agent.summary (20 points)
    let has_summary = memory
        .metadata
        .get("agent")
        .and_then(|a| a.get("summary"))
        .is_some_and(|s| s.as_str().is_some_and(|v| !v.is_empty()));
    if has_summary {
        score += 20;
        notes.push("agent.summary present.".to_string());
    } else {
        notes.push("agent.summary missing.".to_string());
    }

    // Metadata has agent.entities (15 points)
    let has_entities = memory
        .metadata
        .get("agent")
        .and_then(|a| a.get("entities"))
        .is_some_and(|e| e.is_array());
    if has_entities {
        score += 15;
        notes.push("agent.entities present.".to_string());
    } else {
        notes.push("agent.entities missing.".to_string());
    }

    // Metadata has agent.topics (15 points)
    let has_topics = memory
        .metadata
        .get("agent")
        .and_then(|a| a.get("topics"))
        .is_some_and(|t| t.is_array());
    if has_topics {
        score += 15;
        notes.push("agent.topics present.".to_string());
    } else {
        notes.push("agent.topics missing.".to_string());
    }

    let passed = score >= 50;
    EvalResult {
        name: "Memory Structure".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Test 5: Query / Whisper Quality
// ---------------------------------------------------------------------------

async fn test_query_quality(
    query_svc: &nexus_agent::QueryService,
    ingest: &nexus_agent::IngestService,
    memory_repo: &MemoryRepository,
    relation_repo: &MemoryRelationRepository<'_>,
    namespace_id: i64,
) -> EvalResult {
    // Store 3 specific memories with unique, queryable content
    let memories_to_store = [
        (
            "The project uses sqlx version 0.8 for all database operations, \
             with SQLite as the primary backend. Migrations are run automatically.",
            "eval-query-1",
        ),
        (
            "Error handling follows a fail-closed policy: if LLM enrichment fails, \
             memories are buffered for retry rather than silently dropped.",
            "eval-query-2",
        ),
        (
            "The embedding pipeline uses ONNX Runtime with the all-MiniLM-L6-v2 model \
             to produce 384-dimensional vectors for semantic search.",
            "eval-query-3",
        ),
    ];

    for (content, source) in &memories_to_store {
        if let Err(e) = ingest
            .ingest(content, source, namespace_id, memory_repo)
            .await
        {
            return EvalResult {
                name: "Query / Whisper Quality".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Failed to store memory '{}': {}", source, e),
            };
        }
    }

    // Query with a specific question
    let question = "What happens when LLM enrichment fails for a memory?";

    let answer = match query_svc
        .query(question, namespace_id, memory_repo, relation_repo)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            return EvalResult {
                name: "Query / Whisper Quality".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Query failed: {}", e),
            };
        }
    };

    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    // Answer non-empty (30 points)
    if !answer.answer.trim().is_empty() {
        score += 30;
        notes.push(format!("Answer: {} chars.", answer.answer.len()));
    } else {
        notes.push("Answer is empty.".to_string());
    }

    // Citations present (25 points)
    if !answer.citations.is_empty() {
        score += 25;
        notes.push(format!("{} citation(s) provided.", answer.citations.len()));
    } else {
        notes.push("No citations provided.".to_string());
    }

    // Answer references the relevant concept (25 points)
    let answer_lower = answer.answer.to_lowercase();
    if answer_lower.contains("buffer")
        || answer_lower.contains("retry")
        || answer_lower.contains("fail")
        || answer_lower.contains("enrichment")
    {
        score += 25;
        notes.push("Answer references relevant concept.".to_string());
    } else {
        notes.push("Answer may not address the question.".to_string());
    }

    // Confidence > 0 (20 points)
    if answer.confidence > 0.0 {
        score += 20;
        notes.push(format!("Confidence: {:.2}.", answer.confidence));
    } else {
        notes.push("Confidence is zero.".to_string());
    }

    let passed = score >= 50;
    EvalResult {
        name: "Query / Whisper Quality".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Test 6: Consolidation / Dream Quality
// ---------------------------------------------------------------------------

async fn test_consolidation_quality(
    consolidate_svc: &nexus_agent::ConsolidateService,
    memory_repo: &MemoryRepository,
    relation_repo: &MemoryRelationRepository<'_>,
    namespace_id: i64,
) -> EvalResult {
    // By this point, tests 2-5 have stored 6+ unconsolidated memories.
    // Run consolidation on them.
    let result = match consolidate_svc
        .consolidate(namespace_id, memory_repo, relation_repo)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return EvalResult {
                name: "Consolidation / Dream".to_string(),
                passed: false,
                score: 0,
                max_score: 100,
                details: format!("Consolidation failed: {}", e),
            };
        }
    };

    let mut score = 0u32;
    let mut notes: Vec<String> = Vec::new();

    // Consolidation happened (returned Some) (30 points)
    match result {
        Some(count) => {
            score += 30;
            notes.push(format!("Consolidated {} memories.", count));
        }
        None => {
            notes.push(
                "No consolidation performed (not enough unconsolidated memories).".to_string(),
            );
            return EvalResult {
                name: "Consolidation / Dream".to_string(),
                passed: false,
                score: 30,
                max_score: 100,
                details: notes.join(" "),
            };
        }
    }

    // Check that source memories were marked as consolidated (30 points)
    let unconsolidated = match memory_repo.get_unconsolidated(namespace_id, 100).await {
        Ok(memories) => memories,
        Err(e) => {
            notes.push(format!("Could not check consolidation marks: {}", e));
            return EvalResult {
                name: "Consolidation / Dream".to_string(),
                passed: score >= 50,
                score,
                max_score: 100,
                details: notes.join(" "),
            };
        }
    };

    // All previously unconsolidated memories should now be marked
    // (new unconsolidated count should be 0 or very low)
    if unconsolidated.is_empty() {
        score += 30;
        notes.push("All source memories marked consolidated.".to_string());
    } else {
        // Some might remain if batch_size limited consolidation
        score += 15;
        notes.push(format!(
            "{} memories remain unconsolidated (may be batch-limited).",
            unconsolidated.len()
        ));
    }

    // Check relations were created (20 points)
    // Pick a random memory ID from the consolidated set and check for relations
    let all_memories = match memory_repo.search_by_namespace(namespace_id, 100, 0).await {
        Ok(m) => m,
        Err(_) => {
            notes.push("Could not verify relations.".to_string());
            return EvalResult {
                name: "Consolidation / Dream".to_string(),
                passed: score >= 50,
                score,
                max_score: 100,
                details: notes.join(" "),
            };
        }
    };

    let mut has_relations = false;
    for mem in &all_memories {
        let related = relation_repo.get_related(mem.id).await.unwrap_or_default();
        if !related.is_empty() {
            has_relations = true;
            notes.push(format!(
                "Memory #{} has {} relation(s).",
                mem.id,
                related.len()
            ));
            break;
        }
    }

    if has_relations {
        score += 20;
    } else {
        notes.push("No relations found between memories.".to_string());
    }

    // Check that consolidated metadata was set (20 points)
    let mut has_consolidated_meta = false;
    for mem in &all_memories {
        let consolidated = mem
            .metadata
            .get("agent")
            .and_then(|a| a.get("consolidated"))
            .and_then(|c| c.as_bool());
        if consolidated == Some(true) {
            has_consolidated_meta = true;
            break;
        }
    }

    if has_consolidated_meta {
        score += 20;
        notes.push("Consolidation metadata set correctly.".to_string());
    } else {
        notes.push("Consolidation metadata not found.".to_string());
    }

    let passed = score >= 50;
    EvalResult {
        name: "Consolidation / Dream".to_string(),
        passed,
        score,
        max_score: 100,
        details: notes.join(" "),
    }
}
