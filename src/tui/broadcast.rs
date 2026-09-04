//! One command, many hosts. SPEC §6.5.
//!
//! v2 makes this the only multi-host *action* on the dashboard, in place of
//! everything the fleet-consensus screen used to be. The argument is a good
//! one: "are these machines configured the same" is a question you occasionally
//! ask, and "reload nginx on all the web boxes" is a thing you do all day.
//!
//! `essh run <tag> -- <cmd>` already fans out over one-shot connections and
//! prints to stdout. What is new here is reporting into a frame that has to
//! keep drawing: a broadcast that blocks the event loop makes the UI look
//! like the command hung, on every host, including the ones that already
//! answered.
//!
//! So this holds only state. The connecting and running belong to the event
//! loop, which owns the runtime; this owns what is on screen and cannot
//! accidentally do IO.

use std::time::{Duration, Instant};

/// One host's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// Sent, nothing back yet. Rendered as running rather than as silence,
    /// because a host that is slow and a host that is unreachable look the
    /// same until one of them answers.
    Running,
    Done {
        /// `None` when the server closed the channel without sending a
        /// status. The row says `finished` rather than inventing a zero.
        exit: Option<u32>,
        took: Duration,
        /// First line of output, if any. The panel has one row per host, so
        /// this is a hint rather than a transcript.
        first_line: Option<String>,
    },
    Failed(String),
}

impl Reply {
    pub fn is_running(&self) -> bool {
        matches!(self, Reply::Running)
    }

    /// The right-hand column: what happened, in the fewest words that are
    /// still true.
    pub fn summary(&self) -> String {
        match self {
            Reply::Running => "running".into(),
            Reply::Done { exit, took, .. } => match exit {
                Some(code) => format!("exit {code} · {}ms", took.as_millis()),
                None => format!("finished · {}ms", took.as_millis()),
            },
            Reply::Failed(why) => why.clone(),
        }
    }

    pub fn ok(&self) -> bool {
        matches!(self, Reply::Done { exit: Some(0), .. })
    }
}

#[derive(Default)]
pub struct Broadcast {
    /// Tag the command goes to, as `key=value` or a bare value.
    pub target: String,
    /// What is typed, or was sent.
    pub command: String,
    /// True while the input line has the caret.
    pub editing: bool,
    /// Which field the caret is in.
    pub editing_target: bool,
    pub replies: Vec<(String, Reply)>,
    pub started: Option<Instant>,
}

impl Broadcast {
    /// Begin a run against `hosts`, every one marked running up front.
    ///
    /// Marked before anything is sent, so the panel shows the whole target
    /// set from the first frame. Filling it in as replies arrive would make a
    /// broadcast to eight hosts look like a broadcast to however many had
    /// answered so far.
    pub fn start(&mut self, hosts: &[String]) {
        self.replies = hosts.iter().map(|h| (h.clone(), Reply::Running)).collect();
        self.started = Some(Instant::now());
        self.editing = false;
    }

    pub fn record(&mut self, host: &str, reply: Reply) {
        if let Some(slot) = self.replies.iter_mut().find(|(h, _)| h == host) {
            slot.1 = reply;
        }
    }

    pub fn running(&self) -> usize {
        self.replies.iter().filter(|(_, r)| r.is_running()).count()
    }

    pub fn in_flight(&self) -> bool {
        self.running() > 0
    }

    /// The panel's subtitle. Counts what happened, and says so in words that
    /// agree with their nouns.
    pub fn status(&self) -> String {
        if self.replies.is_empty() {
            return "type once, many hosts".into();
        }
        let running = self.running();
        if running > 0 {
            return format!(
                "{} of {}",
                crate::format::count(running, "host"),
                self.replies.len()
            );
        }
        let failed = self.replies.iter().filter(|(_, r)| !r.ok()).count();
        if failed == 0 {
            format!("{} ok", crate::format::count(self.replies.len(), "host"))
        } else {
            format!(
                "{} of {} failed",
                failed,
                crate::format::count(self.replies.len(), "host")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broadcast to eight hosts is a broadcast to eight hosts on the first
    /// frame, not a list that grows as replies arrive.
    #[test]
    fn every_target_is_listed_before_anything_answers() {
        let mut b = Broadcast::default();
        let hosts: Vec<String> = ["web-01", "web-02", "web-03"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        b.start(&hosts);
        assert_eq!(b.replies.len(), 3);
        assert!(b.replies.iter().all(|(_, r)| r.is_running()));
        assert_eq!(b.status(), "3 hosts of 3");
    }

    #[test]
    fn the_status_agrees_with_its_noun() {
        let mut b = Broadcast::default();
        b.start(&["web-01".into()]);
        assert_eq!(b.status(), "1 host of 1");
        b.record(
            "web-01",
            Reply::Done {
                exit: Some(0),
                took: Duration::from_millis(40),
                first_line: None,
            },
        );
        assert_eq!(b.status(), "1 host ok");
    }

    /// A non-zero exit is not a failure to run, and a failure to connect is
    /// not an exit code. The panel has to be able to say which.
    #[test]
    fn a_failed_host_is_counted_and_named() {
        let mut b = Broadcast::default();
        b.start(&["web-01".into(), "web-02".into()]);
        b.record(
            "web-01",
            Reply::Done {
                exit: Some(1),
                took: Duration::from_millis(12),
                first_line: Some("nginx: configuration file test failed".into()),
            },
        );
        b.record("web-02", Reply::Failed("connection refused".into()));
        assert!(!b.in_flight());
        assert_eq!(b.status(), "2 of 2 hosts failed");
        assert_eq!(b.replies[0].1.summary(), "exit 1 · 12ms");
        assert_eq!(b.replies[1].1.summary(), "connection refused");
    }

    #[test]
    fn a_reply_for_a_host_that_is_not_a_target_is_dropped() {
        let mut b = Broadcast::default();
        b.start(&["web-01".into()]);
        b.record("db-09", Reply::Failed("nope".into()));
        assert_eq!(b.replies.len(), 1);
        assert!(b.replies[0].1.is_running());
    }
}
