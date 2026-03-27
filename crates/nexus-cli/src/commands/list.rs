//! List command - list memories with time-range and category filters

use anyhow::Result;
use chrono::{Duration, Utc};
use nexus_core::Config;
use nexus_storage::repository::ListMemoryFilters;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

pub async fn execute(
    agent: String,
    category: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: usize,
    offset: usize,
    include_raw: bool,
) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            println!("No memories found for agent '{}'", agent);
            return Ok(());
        }
    };

    let since_dt = parse_time_filter(since.as_deref())?;
    let until_dt = parse_time_filter(until.as_deref())?;

    let memories = memory_repo
        .list_filtered(
            namespace.id,
            ListMemoryFilters {
                category: category.as_deref(),
                since: since_dt,
                until: until_dt,
                content_like: None,
                include_raw,
                limit: limit as i64,
                offset: offset as i64,
            },
        )
        .await?;

    let count = memory_repo
        .count_filtered(
            namespace.id,
            category.as_deref(),
            since_dt,
            until_dt,
            include_raw,
        )
        .await?;

    if memories.is_empty() {
        println!("No memories found matching filters for agent '{}'", agent);
        return Ok(());
    }

    println!(
        "Showing {}/{} memories for '{}':\n",
        memories.len(),
        count,
        agent
    );

    for memory in &memories {
        println!("──────────────────────────────────────");
        println!(
            "ID: {} | {} | {}",
            memory.id,
            memory.category,
            memory.created_at.format("%Y-%m-%d %H:%M")
        );
        if !memory.labels.is_empty() {
            println!("  Labels: {}", memory.labels.join(", "));
        }
        let preview: String = memory.content.chars().take(300).collect();
        if memory.content.chars().count() > 300 {
            println!("  {preview}...");
        } else {
            println!("  {preview}");
        }
        println!();
    }

    if (offset + memories.len()) < count as usize {
        println!(
            "... {} more. Use --offset {} to see next page.",
            count as usize - offset - memories.len(),
            offset + limit
        );
    }

    Ok(())
}
pub(crate) fn parse_time_filter(s: Option<&str>) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let s = match s {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(None),
    };

    // Relative: Nm, Nh, Nd, Nw
    if let Some(n) = s.strip_suffix('m') {
        let minutes: i64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid minutes: {}", s))?;
        return Ok(Some(Utc::now() - Duration::minutes(minutes)));
    }
    if let Some(n) = s.strip_suffix('h') {
        let hours: i64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid hours: {}", s))?;
        return Ok(Some(Utc::now() - Duration::hours(hours)));
    }
    if let Some(n) = s.strip_suffix('d') {
        let days: i64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid days: {}", s))?;
        return Ok(Some(Utc::now() - Duration::days(days)));
    }
    if let Some(n) = s.strip_suffix('w') {
        let weeks: i64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid weeks: {}", s))?;
        return Ok(Some(Utc::now() - Duration::weeks(weeks)));
    }

    // ISO 8601 or date
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| Some(dt.with_timezone(&Utc)))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| Some(ndt.and_utc()))
        })
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|nd| Some(nd.and_hms_opt(0, 0, 0).unwrap().and_utc()))
        })
        .map_err(|_| anyhow::anyhow!("Cannot parse '{}'. Use Nm/Nh/Nd/Nw or YYYY-MM-DD", s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_parse_time_filter_none() {
        assert!(parse_time_filter(None).unwrap().is_none());
        assert!(parse_time_filter(Some("")).unwrap().is_none());
        assert!(parse_time_filter(Some("  ")).unwrap().is_none());
    }

    #[test]
    fn test_parse_time_filter_minutes() {
        let result = parse_time_filter(Some("30m")).unwrap().unwrap();
        let expected = Utc::now() - Duration::minutes(30);
        let diff = (expected - result).num_seconds().abs();
        assert!(diff < 2, "Minutes parse should be within 2s of expected");
    }

    #[test]
    fn test_parse_time_filter_hours() {
        let result = parse_time_filter(Some("2h")).unwrap().unwrap();
        let expected = Utc::now() - Duration::hours(2);
        let diff = (expected - result).num_seconds().abs();
        assert!(diff < 2);
    }

    #[test]
    fn test_parse_time_filter_days() {
        let result = parse_time_filter(Some("7d")).unwrap().unwrap();
        let expected = Utc::now() - Duration::days(7);
        let diff = (expected - result).num_seconds().abs();
        assert!(diff < 2);
    }

    #[test]
    fn test_parse_time_filter_weeks() {
        let result = parse_time_filter(Some("2w")).unwrap().unwrap();
        let expected = Utc::now() - Duration::weeks(2);
        let diff = (expected - result).num_seconds().abs();
        assert!(diff < 2);
    }

    #[test]
    fn test_parse_time_filter_iso8601() {
        let result = parse_time_filter(Some("2026-03-24T12:00:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(result.timestamp(), 1774353600);
    }

    #[test]
    fn test_parse_time_filter_date_only() {
        let result = parse_time_filter(Some("2026-03-24")).unwrap().unwrap();
        assert_eq!(result.date_naive().to_string(), "2026-03-24");
        // Should be midnight UTC
        assert_eq!(result.time().hour(), 0);
        assert_eq!(result.time().minute(), 0);
    }

    #[test]
    fn test_parse_time_filter_datetime() {
        let result = parse_time_filter(Some("2026-03-24 14:30:00"))
            .unwrap()
            .unwrap();
        assert_eq!(result.date_naive().to_string(), "2026-03-24");
        assert_eq!(result.time().hour(), 14);
        assert_eq!(result.time().minute(), 30);
    }

    #[test]
    fn test_parse_time_filter_invalid() {
        assert!(parse_time_filter(Some("not-a-date")).is_err());
        assert!(parse_time_filter(Some("abc")).is_err());
    }

    #[test]
    fn test_parse_time_filter_whitespace_trimmed() {
        // Whitespace should be trimmed
        let result = parse_time_filter(Some("  1h  ")).unwrap().unwrap();
        let expected = Utc::now() - Duration::hours(1);
        let diff = (expected - result).num_seconds().abs();
        assert!(diff < 2);
    }
}
