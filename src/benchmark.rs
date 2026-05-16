use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::AppConfig;

/// Captures benchmark metadata and phase timings for one CLI command.
#[derive(Debug)]
pub struct BenchmarkRun {
    command: String,
    fields: BTreeMap<String, Value>,
    log_path: PathBuf,
    phases: BTreeMap<String, f64>,
    started_at: chrono::DateTime<Utc>,
    started: Instant,
}

impl BenchmarkRun {
    /// Creates a benchmark run when benchmarking is enabled in the config.
    pub fn from_config(command: impl Into<String>, config: &AppConfig) -> Option<Self> {
        config.benchmark.enabled.then(|| Self {
            command: command.into(),
            fields: BTreeMap::new(),
            log_path: config.benchmark_log_path(),
            phases: BTreeMap::new(),
            started_at: Utc::now(),
            started: Instant::now(),
        })
    }

    /// Adds command metadata to the benchmark record.
    pub fn set_field(&mut self, key: impl Into<String>, value: impl Serialize) {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.fields.insert(key.into(), value);
    }

    /// Records the elapsed time for one named phase in milliseconds.
    pub fn record_phase(&mut self, name: impl Into<String>, elapsed: Duration) {
        self.phases.insert(name.into(), duration_ms(elapsed));
    }

    /// Appends this benchmark run as one JSONL record.
    pub fn finish(&self) -> Result<()> {
        if let Some(parent) = self.log_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create benchmark log directory: {}",
                    parent.display()
                )
            })?;
        }

        let mut record = Map::new();
        record.insert(
            "timestamp".to_string(),
            json!(self.started_at.to_rfc3339_opts(SecondsFormat::Millis, true)),
        );
        record.insert("command".to_string(), json!(&self.command));
        record.insert(
            "total_ms".to_string(),
            json!(duration_ms(self.started.elapsed())),
        );
        record.insert("phases".to_string(), json!(&self.phases));
        for (key, value) in &self.fields {
            record.insert(key.clone(), value.clone());
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .with_context(|| {
                format!("failed to open benchmark log: {}", self.log_path.display())
            })?;
        serde_json::to_writer(&mut file, &record).with_context(|| {
            format!(
                "failed to serialize benchmark log: {}",
                self.log_path.display()
            )
        })?;
        writeln!(file).with_context(|| {
            format!("failed to write benchmark log: {}", self.log_path.display())
        })?;
        Ok(())
    }
}

/// Runs an operation and records its elapsed time as a benchmark phase.
pub fn time_phase<T>(
    benchmark: &mut Option<&mut BenchmarkRun>,
    name: impl Into<String>,
    operation: impl FnOnce() -> T,
) -> T {
    let started = Instant::now();
    let result = operation();
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.record_phase(name, started.elapsed());
    }
    result
}

/// Runs a command body with optional benchmark logging.
pub fn measure<T>(
    config: &AppConfig,
    command: &str,
    configure: impl FnOnce(&mut BenchmarkRun),
    operation: impl FnOnce(Option<&mut BenchmarkRun>) -> Result<T>,
) -> Result<T> {
    let mut benchmark = BenchmarkRun::from_config(command, config);
    if let Some(benchmark) = benchmark.as_mut() {
        configure(benchmark);
    }

    let result = operation(benchmark.as_mut());
    if let Some(benchmark) = benchmark.as_mut() {
        match &result {
            Ok(_) => benchmark.set_field("status", "ok"),
            Err(error) => {
                benchmark.set_field("status", "error");
                benchmark.set_field("error", error.to_string());
            }
        }
        if let Err(error) = benchmark.finish() {
            eprintln!("warning: failed to write benchmark log: {error:#}");
        }
    }
    result
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
