//! LLM prompt templates for agent operations

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

Guidelines:
1. Synthesize a clear, accurate answer using the provided memories
2. Cite specific memory IDs when referencing information [Memory #ID]
3. If the context doesn't contain enough information, say so
4. Be concise but thorough

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
    "confidence": float between 0.0 and 1.0
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
