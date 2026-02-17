//! Benchmarks for nexus-orchestrator
//!
//! Performance targets:
//! - Event propagation: <1ms
//! - Concurrent sessions: 10,000+
//! - Session creation: <100μs

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use nexus_orchestrator::{
    Event, EventBus, EventType, Orchestrator, OrchestratorConfig, SessionConfig, SessionManager,
};
use std::time::Duration;

fn bench_session_creation(c: &mut Criterion) {
    let manager = SessionManager::new(SessionConfig {
        max_sessions: 100_000,
        ..Default::default()
    });
    manager.initialize().unwrap();

    c.bench_function("session_creation", |b| {
        b.iter(|| {
            let session = manager.create_session("test-agent").unwrap();
            // Clean up to avoid hitting limits
            let _ = manager.end_session(&session.id);
        });
    });
}

fn bench_event_propagation(c: &mut Criterion) {
    let bus = EventBus::new(1000);
    let mut rx = bus.subscribe();

    c.bench_function("event_propagation", |b| {
        b.iter(|| {
            let event = Event::new(EventType::SessionStarted);
            bus.publish(event).unwrap();
            rx.try_recv().ok()
        });
    });
}

fn bench_concurrent_sessions(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_sessions");

    for size in [100, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let manager = SessionManager::new(SessionConfig {
                    max_sessions: size + 100,
                    ..Default::default()
                });
                manager.initialize().unwrap();

                // Create sessions
                let mut sessions = Vec::with_capacity(size);
                for i in 0..size {
                    sessions.push(
                        manager
                            .create_session(&format!("agent-{}", i % 10))
                            .unwrap(),
                    );
                }

                // Verify count
                assert!(manager.session_count() >= size);

                // Clean up
                for session in sessions {
                    let _ = manager.end_session(&session.id);
                }
            });
        });
    }

    group.finish();
}

fn bench_orchestrator_full(c: &mut Criterion) {
    let orchestrator = Orchestrator::new(OrchestratorConfig::default());
    orchestrator.initialize().unwrap();

    c.bench_function("orchestrator_full_lifecycle", |b| {
        b.iter(|| {
            // Create session
            let session = orchestrator.create_session("benchmark-agent").unwrap();

            // Record activity
            let _ = orchestrator.record_activity(&session.id);

            // End session
            let _ = orchestrator.end_session(&session.id);
        });
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(5))
        .sample_size(100);
    targets = bench_session_creation,
              bench_event_propagation,
              bench_concurrent_sessions,
              bench_orchestrator_full
}

criterion_main!(benches);
