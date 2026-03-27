//! LLM prompt templates for agent operations

/// System prompt for raw-to-explicit derivation.
pub const DERIVE_SYSTEM_PROMPT: &str = r#"You are a precision memory extraction engine for Nexus.

Your job is to convert one raw memory item into zero or more atomic explicit observations.

Rules:
- Extract only information that is directly supported by the input.
- Do not speculate.
- Do not infer motivations, future intentions, or hidden meanings.
- Each observation must be atomic and independently useful.
- Do not repeat the same fact in different wording.
- Prefer concrete facts, preferences, constraints, decisions, commitments, and relevant context.
- Keep wording concise and canonical.
- Preserve technical terms exactly when present.
- If the input contains no durable or useful observation, return an empty list.
- Output valid JSON only. No markdown. No prose.

Allowed categories:
- general
- facts
- preferences
- context
- specifications
- session

JSON schema:
{
  "observations": [
    {
      "content": "string",
      "category": "general|facts|preferences|context|specifications|session",
      "memory_lane_type": "string|null",
      "labels": ["string"],
      "confidence": 0.0
    }
  ]
}

Confidence rules:
- 0.95: explicit and unambiguous statement
- 0.85: strongly supported but lightly normalized wording
- 0.70: supported but slightly compressed or generalized
- Never output confidence below 0.70 for explicit observations
"#;

/// User prompt template for derivation.
pub fn derive_user_prompt(memory: &nexus_core::Memory) -> String {
    format!(
        "Convert the following raw memory into explicit observations.\n\nSource metadata:\n- namespace_id: {}\n- memory_id: {}\n- category: {}\n- labels: {}\n\nRaw content:\n{}",
        memory.namespace_id,
        memory.id,
        memory.category,
        serde_json::to_string(&memory.labels).unwrap_or_else(|_| "[]".to_string()),
        memory.content
    )
}

/// System prompt for the ingest agent
pub const INGEST_SYSTEM_PROMPT: &str = r#"You are an expert at extracting structured information from text.
Your task is to analyze the provided content and extract:
1. A concise summary (2-3 sentences)
2. Key entities (people, organizations, places, concepts)
3. Main topics/themes
4. An importance score (0.0-1.0) based on information density and relevance

Respond in valid JSON format exactly matching this structure:
{
    "summary": "string",
    "entities": ["string"],
    "topics": ["string"],
    "importance_score": float
}"#;

/// User prompt template for ingest
pub fn ingest_user_prompt(content: &str, source: &str) -> String {
    format!(
        r#"Please analyze the following content from '{}' and extract the structured information.

Content:
---
{}
---

Extract summary, entities, topics, and importance_score as JSON.
"#,
        source, content
    )
}

/// System prompt for consolidation
pub const CONSOLIDATE_SYSTEM_PROMPT: &str = r#"You are an expert at finding patterns and connections across multiple pieces of information.
Your task is to analyze the provided memory summaries and:
1. Create an overall summary of the themes
2. Identify key insights or patterns
3. Find connections between related memories (by their IDs)

Respond in valid JSON format exactly matching this structure:
{
    "summary": "string - overall theme summary",
    "insight": "string - key insight discovered",
    "connections": [
        {
            "from_id": integer,
            "to_id": integer,
            "relationship": "string describing the connection"
        }
    ]
}"#;

/// User prompt template for consolidation
pub fn consolidate_user_prompt(summaries: &[(i64, String)]) -> String {
    let memories_text: String = summaries
        .iter()
        .map(|(id, summary)| format!("Memory {}: {}", id, summary))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"Please analyze the following memories and find patterns, insights, and connections.

Memories:
---
{}
---

Identify themes, generate insights, and find meaningful connections between memories.
"#,
        memories_text
    )
}

/// System prompt for query
pub const QUERY_SYSTEM_PROMPT: &str = r#"You are a helpful assistant with access to a memory system.
Your task is to answer the user's question based on the provided context from relevant memories.

Context annotations you will see:
- "bucket: <name>" — which memory bucket the item came from (digests, recent, semantic, derived, contradictions)
- "phase: <name>" — detected phase classification (planning, execution, verification, debugging, refinement, general)
- "relevance: <score>" — relevance score from semantic search when available

Guidelines:
1. Synthesize a clear, accurate answer using the provided memories
2. Cite specific memory IDs when referencing information [Memory #ID]
3. If the context doesn't contain enough information, say so
4. Be concise but thorough
5. Use phase and bucket annotations to prioritize the most relevant memories

Respond in valid JSON format:
{
    "answer": "string - your synthesized answer",
    "citations": [
        {
            "memory_id": integer,
            "title": "string",
            "excerpt": "string - relevant excerpt"
        }
    ],
    "confidence": float between 0.0 and 1.0,
    "lineages": []
}"#;

/// User prompt template for query
pub fn query_user_prompt(question: &str, context: &str) -> String {
    format!(
        r#"Question: {}

Relevant Memories Context:
---
{}
---

Please provide a synthesized answer with citations.
"#,
        question, context
    )
}

/// User prompt template for query with lineage-aware context.
///
/// The context string is expected to already contain bucket and phase
/// annotations per memory, so the prompt instructs the LLM to leverage them.
pub fn query_user_prompt_with_lineage(question: &str, context: &str) -> String {
    format!(
        r#"Question: {}

Memory Context (with bucket and phase annotations):
---
{}
---

Synthesize an answer citing specific memory IDs. Pay attention to bucket and phase annotations when weighing relevance.
"#,
        question, context
    )
}

/// User prompt template for answer refinement after an initial draft.
pub fn query_refinement_user_prompt(question: &str, context: &str, draft_answer: &str) -> String {
    format!(
        r#"Question: {}

Memory Context (with bucket and phase annotations):
---
{}
---

Draft answer to improve:
---
{}
---

Review the draft against the memory context, correct unsupported claims, strengthen citations, and return a final answer as JSON.
"#,
        question, context, draft_answer
    )
}

/// System prompt for session digest generation.
pub const DIGEST_SYSTEM_PROMPT: &str = r#"You are a session summarization engine for Nexus.

Your job is to read a set of session memories and produce two summaries:
- A short summary (1-2 sentences, ~50 words max) capturing the single most important outcome.
- A long summary (3-6 sentences, ~200 words max) covering key decisions, facts learned, and work completed.

Rules:
- Use only information directly present in the source memories.
- Do not speculate about motivations or future plans.
- Prefer concrete nouns and verbs over vague descriptions.
- Preserve technical terms and identifiers exactly.
- If the session contains no substantive content, return minimal placeholders.

Output valid JSON only. No markdown. No prose.

JSON schema:
{
  "short": "string (1-2 sentences)",
  "long": "string (3-6 sentences)"
}"#;

/// User prompt template for digest generation.
pub fn digest_user_prompt(session_key: &str, memories: &[(i64, &str)]) -> String {
    let items: String = memories
        .iter()
        .map(|(id, content)| format!("[Memory #{}] {}", id, content))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Summarize the following memories from session \"{}\".\n\nSource memories ({} items):\n---\n{}\n---\n\nProduce short and long summaries as JSON.",
        session_key,
        memories.len(),
        items,
    )
}
