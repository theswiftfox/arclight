use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::Local;
use tokio::sync::mpsc;

/// Maximum number of log lines to keep in the buffer.
const MAX_LOG_LINES: usize = 10_000;
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 4096;
const QUEUE_CAPACITY: usize = 512;
const BATCH_SIZE: usize = 128;
pub const PAGE_SIZE: usize = 200;

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
        let mut text = text;
        if text.len() > MAX_LINE_BYTES {
            let mut end = MAX_LINE_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
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
    pub lines: VecDeque<Arc<LogEntry>>,
    bytes: usize,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(MAX_LOG_LINES),
            bytes: 0,
        }
    }
}

impl LogBuffer {
    pub fn push(&mut self, entry: LogEntry) {
        let size = entry.text.len() + entry.timestamp.len() + entry.source.len();
        while self.lines.len() >= MAX_LOG_LINES || self.bytes + size > MAX_LOG_BYTES {
            let Some(old) = self.lines.pop_front() else {
                return;
            };
            self.bytes -= old.text.len() + old.timestamp.len() + old.source.len();
        }
        self.bytes += size;
        self.lines.push_back(Arc::new(entry));
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        self.lines
            .iter()
            .map(|entry| entry.formatted())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Lossy, bounded log transport. Never wait on the GUI while draining child pipes.
#[derive(Debug, Clone)]
pub struct LogSender {
    sender: mpsc::Sender<LogEntry>,
    dropped: Arc<AtomicUsize>,
}

impl LogSender {
    pub fn send(&self, entry: LogEntry) {
        if let Err(mpsc::error::TrySendError::Full(_)) = self.sender.try_send(entry) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub struct LogReceiver {
    receiver: mpsc::Receiver<LogEntry>,
    dropped: Arc<AtomicUsize>,
}

impl LogReceiver {
    pub async fn next_batch(&mut self) -> Option<Vec<LogEntry>> {
        // An idle receive suspends only this background future. No UI lock or wait.
        let first = self.receiver.recv().await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut entries = Vec::with_capacity(BATCH_SIZE + 1);
        entries.push(first);
        while entries.len() < BATCH_SIZE {
            match self.receiver.try_recv() {
                Ok(entry) => entries.push(entry),
                Err(_) => break,
            }
        }
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            entries.push(LogEntry::new(
                "Arclight",
                format!("Dropped {dropped} log lines: log queue full"),
            ));
        }
        Some(entries)
    }
}

/// Create a bounded log sender/receiver pair.
#[must_use]
pub fn create_log_channel() -> (LogSender, LogReceiver) {
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
    let dropped = Arc::new(AtomicUsize::new(0));
    (
        LogSender {
            sender,
            dropped: dropped.clone(),
        },
        LogReceiver { receiver, dropped },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_flood_is_bounded_and_reports_drops() {
        let (sender, mut receiver) = create_log_channel();
        for _ in 0..QUEUE_CAPACITY + 20 {
            sender.send(LogEntry::new("test", "line".into()));
        }
        assert_eq!(receiver.receiver.len(), QUEUE_CAPACITY);
        let batch = receiver.next_batch().await.unwrap();
        assert_eq!(batch.len(), BATCH_SIZE + 1);
        assert!(batch.last().unwrap().text.contains("Dropped 20"));
    }

    #[tokio::test]
    async fn idle_receiver_remains_usable_and_closes_cleanly() {
        let (sender, mut receiver) = create_log_channel();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), receiver.next_batch())
                .await
                .is_err()
        );
        sender.send(LogEntry::new("test", "after idle".into()));
        drop(sender);
        assert_eq!(receiver.next_batch().await.unwrap()[0].text, "after idle");
        assert!(receiver.next_batch().await.is_none());
    }

    #[test]
    fn buffer_limits_bytes_and_utf8_lines() {
        let mut buffer = LogBuffer::default();
        for _ in 0..MAX_LOG_LINES {
            buffer.push(LogEntry::new("test", "界".repeat(MAX_LINE_BYTES)));
        }
        assert!(buffer.bytes <= MAX_LOG_BYTES);
        assert!(buffer.lines.len() < MAX_LOG_LINES);
        assert!(
            buffer
                .lines
                .iter()
                .all(|entry| entry.text.len() <= MAX_LINE_BYTES)
        );
        buffer.clear();
        assert_eq!(buffer.bytes, 0);
    }
}
