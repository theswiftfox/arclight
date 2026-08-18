use std::collections::VecDeque;

use chrono::Local;
use tokio::sync::mpsc;

/// Maximum number of log lines to keep in the buffer.
const MAX_LOG_LINES: usize = 10_000;

/// A single log entry from a subprocess.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: &'static str,
    pub text: String,
}

impl LogEntry {
    #[must_use]
    pub fn new(source: &'static str, text: String) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source,
            text,
        }
    }

    #[must_use]
    pub fn formatted(&self) -> String {
        format!("[{}] [{}] {}", self.timestamp, self.source, self.text)
    }
}

/// Log buffer with a fixed capacity.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    pub lines: VecDeque<LogEntry>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(MAX_LOG_LINES),
        }
    }
}

impl LogBuffer {
    pub fn push(&mut self, entry: LogEntry) {
        if self.lines.len() >= MAX_LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(entry);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn to_text(&self) -> String {
        self.lines
            .iter()
            .map(LogEntry::formatted)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Create a log sender/receiver pair.
#[must_use]
pub fn create_log_channel() -> (
    mpsc::UnboundedSender<LogEntry>,
    mpsc::UnboundedReceiver<LogEntry>,
) {
    mpsc::unbounded_channel()
}
