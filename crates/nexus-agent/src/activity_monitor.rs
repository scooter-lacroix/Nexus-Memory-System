//! Activity monitoring and sleep detection for dream cycle calibration.

use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::debug;

/// Tracks user activity to detect idle periods and ideal times for deep dreaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityMonitor {
    /// Sampled activity timestamps (rounded to 10 min, retained 7 days)
    pub activity_log: Vec<DateTime<Utc>>,
    /// Automatically detected typical sleep hour (0-23)
    pub detected_sleep_hour: Option<u8>,
    /// When the last deep dream was successfully completed
    pub last_deep_dream: Option<DateTime<Utc>>,
    /// Minimum time between deep dreams
    #[serde(with = "serde_duration_secs")]
    pub deep_dream_cooldown: Duration,
}

impl Default for ActivityMonitor {
    fn default() -> Self {
        Self {
            activity_log: Vec::new(),
            detected_sleep_hour: None,
            last_deep_dream: None,
            deep_dream_cooldown: Duration::hours(24),
        }
    }
}

impl ActivityMonitor {
    /// Record a sample of user activity.
    pub fn record_activity(&mut self) {
        let now = Utc::now();
        // Round to 10 minute window for sampling efficiency
        let rounded = DateTime::<Utc>::from_naive_utc_and_offset(
            now.naive_utc()
                .with_nanosecond(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_minute((now.minute() / 10) * 10)
                .unwrap(),
            Utc,
        );

        if !self.activity_log.contains(&rounded) {
            self.activity_log.push(rounded);
            // Retain only 7 days
            let week_ago = now - Duration::days(7);
            self.activity_log.retain(|t| *t > week_ago);
            debug!("Activity recorded at {}", rounded);
        }

        // Recompute sleep hour from updated activity log
        self.detected_sleep_hour = self.compute_sleep_hour();
    }

    /// Compute the most likely sleep hour from the activity log.
    ///
    /// Counts activity samples per UTC hour (0–23) and returns the
    /// hour with the *fewest* samples — the user is typically away.
    /// Returns `None` when the log has fewer than 14 entries (insufficient data).
    pub fn compute_sleep_hour(&self) -> Option<u8> {
        if self.activity_log.len() < 14 {
            return None;
        }

        let mut hour_counts = [0usize; 24];
        for ts in &self.activity_log {
            let h = ts.hour() as usize;
            hour_counts[h] += 1;
        }

        // Find the hour with minimum activity (earliest on tie).
        let (sleep_hour, _min_count) = hour_counts
            .iter()
            .enumerate()
            .min_by_key(|&(h, &count)| (count, h))
            .expect("24 elements guaranteed");

        Some(sleep_hour as u8)
    }
    /// Check if deep dream conditions are met.
    ///
    /// When `detected_sleep_hour` matches the current hour (±2h window),
    /// the inactivity threshold is relaxed from 30 min to 10 min since
    /// the user is likely away during their sleep window.
    pub fn should_deep_dream(&self) -> bool {
        let now = Utc::now();

        // 1. Cooldown check
        if let Some(last) = self.last_deep_dream {
            if now < last + self.deep_dream_cooldown {
                return false;
            }
        }

        // 2. Inactivity check — threshold depends on sleep window
        let in_sleep_window = self.detected_sleep_hour.is_some_and(|sleep_h| {
            let current_h = now.hour() as i32;
            let sleep_h = sleep_h as i32;
            let diff = (current_h - sleep_h).rem_euclid(24);
            diff <= 2 || diff >= 22
        });

        let inactivity_threshold = if in_sleep_window {
            Duration::minutes(10)
        } else {
            Duration::minutes(30)
        };

        if let Some(last_active) = self.activity_log.last() {
            if now < *last_active + inactivity_threshold {
                return false;
            }
        }

        true
    }

    /// Default path for global activity monitor persistence.
    pub fn persistence_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("nexus-memory-system")
            .join("activity_monitor.json")
    }

    /// Load from disk.
    pub fn load() -> Self {
        let path = Self::persistence_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(monitor) = serde_json::from_str(&content) {
                    return monitor;
                }
            }
        }
        Self::default()
    }

    /// Save to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

mod serde_duration_secs {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(duration.num_seconds())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = i64::deserialize(deserializer)?;
        Ok(Duration::seconds(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_deduplication() {
        let mut monitor = ActivityMonitor::default();
        monitor.record_activity();
        let len = monitor.activity_log.len();
        monitor.record_activity(); // Same 10min window
        assert_eq!(monitor.activity_log.len(), len);
    }

    #[test]
    fn test_should_deep_dream_cooldown() {
        let mut monitor = ActivityMonitor {
            last_deep_dream: Some(Utc::now() - Duration::hours(12)),
            ..ActivityMonitor::default()
        };
        assert!(!monitor.should_deep_dream());

        monitor.last_deep_dream = Some(Utc::now() - Duration::hours(25));
        // Need an activity log entry to pass inactivity check
        monitor.activity_log.push(Utc::now() - Duration::hours(2));
        assert!(monitor.should_deep_dream());
    }

    #[test]
    fn test_compute_sleep_hour_insufficient_data() {
        let monitor = ActivityMonitor::default();
        assert_eq!(monitor.compute_sleep_hour(), None);
    }

    #[test]
    fn test_compute_sleep_hour_from_work_hours() {
        let mut monitor = ActivityMonitor::default();
        // Add activities only during hours 9-17 (work hours) over multiple days
        for day_offset in 0..7 {
            for hour in 9..=17_u32 {
                let base = Utc::now() - Duration::days(day_offset);
                let ts = DateTime::<Utc>::from_naive_utc_and_offset(
                    base.naive_utc()
                        .with_hour(hour)
                        .unwrap()
                        .with_minute(0)
                        .unwrap()
                        .with_second(0)
                        .unwrap(),
                    Utc,
                );
                monitor.activity_log.push(ts);
            }
        }
        // 63 entries, well above the 14 minimum
        let sleep_hour = monitor.compute_sleep_hour().unwrap();
        // Should pick a non-work hour (0-8 or 18-23)
        assert!(
            !(9..=17).contains(&sleep_hour),
            "Expected sleep hour outside 9-17, got {}",
            sleep_hour
        );
    }
}
