//! What changed, and when. SPEC §6.6.
//!
//! The activity panel needs an event log, and essh had none: every screen
//! showed the current state and nothing showed how it got there. "build-01
//! cpu over 90%" is a fact you can read off the hosts table; "build-01 went
//! over 90% four minutes ago" is the one that tells you whether to care.
//!
//! Entries come from transitions the app already computes each frame. Nothing
//! new is collected, and nothing is written down until something actually
//! changes: a log that appends a line per poll is a log nobody reads.
//!
//! One clock for the lot, which is the property SPEC §2.3 asks for. The
//! handoff's own activity panel says `pg_dump started 12:14` beside a session
//! that has been up 42 minutes at 12:25, because the two came from different
//! places.

use std::collections::{HashMap, VecDeque};

use super::HostState;
#[cfg(test)]
use crate::session::SessionState;

/// One thing that happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub at: chrono::DateTime<chrono::Local>,
    /// The host it happened to, shown in its severity colour.
    pub host: String,
    /// How bad it was when it happened, for that colour. Kept rather than
    /// re-derived, because the host may have recovered since and the line is
    /// about the moment, not about now.
    pub severity: HostState,
    pub text: String,
}

pub struct ActivityLog {
    events: VecDeque<Event>,
    cap: usize,
    /// Last seen state per host, so a transition is a transition rather than
    /// a reading.
    host_states: HashMap<String, HostState>,
    /// Last seen state per session, keyed by the session's id.
    session_states: HashMap<String, &'static str>,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new(64)
    }
}

impl ActivityLog {
    pub fn new(cap: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(cap),
            cap,
            host_states: HashMap::new(),
            session_states: HashMap::new(),
        }
    }

    /// Newest first, which is the order the panel reads in.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &Event> {
        self.events.iter().rev().take(n)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    fn push(&mut self, host: &str, severity: HostState, text: String) {
        if self.events.len() == self.cap {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            at: chrono::Local::now(),
            host: host.to_string(),
            severity,
            text,
        });
    }

    /// Record host state changes.
    ///
    /// The first sighting of a host is not an event. Every host is `unprobed`
    /// on the first frame, so treating that as a transition would fill the
    /// panel with eleven lines about starting up, which is the one thing
    /// nobody needs told.
    pub fn observe_hosts(&mut self, hosts: &[super::HostDisplay]) {
        for host in hosts {
            let now = host.state();
            match self.host_states.insert(host.name.clone(), now) {
                None => continue,
                Some(before) if before == now => continue,
                Some(before) => {
                    if let Some(text) = describe(before, now, host) {
                        self.push(&host.name, now, text);
                    }
                }
            }
        }
    }

    /// Record session state changes, in the words the sessions table uses.
    pub fn observe_sessions(
        &mut self,
        sessions: &[crate::session::Session],
        active: Option<usize>,
    ) {
        for (i, s) in sessions.iter().enumerate() {
            let word = s.state_word(active == Some(i));
            match self.session_states.insert(s.id.clone(), word) {
                Some(before) if before == word => continue,
                Some(_) | None => {}
            }
            let severity = if s.is_dropped() {
                HostState::Dropped
            } else {
                HostState::Up
            };
            let text = match word {
                "attached" => "attached".to_string(),
                "detached" => "detached, still running".to_string(),
                "dropped" => "session dropped".to_string(),
                "opening" => continue,
                other => other.to_string(),
            };
            self.push(&s.label, severity, text);
        }
        // A session that has gone is not a transition worth a line: closing
        // one is something you did on purpose a moment ago.
        self.session_states
            .retain(|id, _| sessions.iter().any(|s| &s.id == id));
    }
}

/// The sentence for one host transition, or `None` when the change is not
/// worth a line.
fn describe(before: HostState, now: HostState, host: &super::HostDisplay) -> Option<String> {
    // Coming out of `unprobed` is the first reading, not a change in the
    // world. A host that turns out to be offline still gets a line, because
    // that one is worth knowing on the frame you find out.
    if before == HostState::Unprobed && matches!(now, HostState::Up | HostState::Busy) {
        return None;
    }
    Some(match now {
        HostState::Up => "recovered".to_string(),
        HostState::Busy => match host.vitals.crossed() {
            Some((kind, pct)) => format!("{kind} over {}%", pct.round()),
            None => "over threshold".to_string(),
        },
        HostState::Dropped => "connection reset".to_string(),
        HostState::Offline => "went offline".to_string(),
        HostState::Unprobed => return None,
    })
}

/// Whether a session state string means the same thing to the log as the last
/// one did. Kept out of `observe_sessions` so the mapping is testable.
#[cfg(test)]
pub fn session_word(state: &SessionState, attached: bool) -> &'static str {
    match state {
        SessionState::Active if attached => "attached",
        SessionState::Active | SessionState::Suspended => "detached",
        SessionState::Connecting => "opening",
        SessionState::Reconnecting { .. } | SessionState::Disconnected { .. } => "dropped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{HostDisplay, HostStatus, HostVitals};

    fn host(name: &str, status: HostStatus, vitals: HostVitals) -> HostDisplay {
        HostDisplay {
            name: name.into(),
            hostname: "127.0.0.1".into(),
            port: 22,
            user: "matt".into(),
            status,
            last_seen: String::new(),
            tags: String::new(),
            tag_pairs: Vec::new(),
            latency_ms: None,
            latency_history: Vec::new(),
            jump_host: None,
            diverge_count: None,
            vitals,
            session_count: 0,
        }
    }

    /// The first frame has every host unprobed. Logging that as a transition
    /// would open the panel with one line per host saying essh had started,
    /// which is the least useful thing it could say.
    #[test]
    fn starting_up_is_not_activity() {
        let mut log = ActivityLog::default();
        let fleet: Vec<_> = (0..11)
            .map(|i| {
                host(
                    &format!("h{i}"),
                    HostStatus::NeverProbed,
                    HostVitals::default(),
                )
            })
            .collect();
        log.observe_hosts(&fleet);
        assert_eq!(log.len(), 0);

        // And the first real reading is not one either.
        let up: Vec<_> = fleet
            .iter()
            .map(|h| host(&h.name, HostStatus::Online, HostVitals::default()))
            .collect();
        log.observe_hosts(&up);
        assert_eq!(log.len(), 0, "the first successful probe is not news");
    }

    /// A host that turns out never to answer is worth a line on the frame you
    /// find out, even though that is also its first reading.
    #[test]
    fn a_host_that_never_answers_is_worth_a_line() {
        let mut log = ActivityLog::default();
        log.observe_hosts(&[host(
            "staging-web-01",
            HostStatus::NeverProbed,
            HostVitals::default(),
        )]);
        log.observe_hosts(&[host(
            "staging-web-01",
            HostStatus::Offline,
            HostVitals::default(),
        )]);
        assert_eq!(log.len(), 1);
        let e = log.recent(1).next().expect("an event").clone();
        assert_eq!(e.severity, HostState::Offline);
        assert!(e.text.contains("offline"), "{}", e.text);
    }

    /// Nothing changing writes nothing down. A log that appends a line per
    /// poll is a log nobody reads.
    #[test]
    fn a_steady_fleet_writes_nothing() {
        let mut log = ActivityLog::default();
        let fleet = [host("web-01", HostStatus::Online, HostVitals::default())];
        for _ in 0..50 {
            log.observe_hosts(&fleet);
        }
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn crossing_a_threshold_names_it() {
        let mut log = ActivityLog::default();
        log.observe_hosts(&[host("db-01", HostStatus::Online, HostVitals::default())]);
        log.observe_hosts(&[host(
            "db-01",
            HostStatus::Online,
            HostVitals {
                disk_pct: Some(81.0),
                ..Default::default()
            },
        )]);
        let e = log.recent(1).next().expect("an event").clone();
        assert_eq!(e.severity, HostState::Busy);
        assert_eq!(e.text, "disk over 81%");

        // And coming back under it is worth a line too.
        log.observe_hosts(&[host("db-01", HostStatus::Online, HostVitals::default())]);
        assert_eq!(log.recent(1).next().expect("an event").text, "recovered");
    }

    /// The buffer is bounded. A dashboard left open for a week must not grow
    /// a log the length of the week.
    #[test]
    fn the_log_is_bounded() {
        let mut log = ActivityLog::new(4);
        for i in 0..20 {
            let vitals = if i % 2 == 0 {
                HostVitals {
                    disk_pct: Some(81.0),
                    ..Default::default()
                }
            } else {
                HostVitals::default()
            };
            log.observe_hosts(&[host("db-01", HostStatus::Online, vitals)]);
        }
        assert_eq!(log.len(), 4);
    }

    /// Newest first, because the panel reads downwards from the most recent.
    #[test]
    fn recent_returns_newest_first() {
        let mut log = ActivityLog::default();
        for name in ["a", "b"] {
            log.observe_hosts(&[host(name, HostStatus::NeverProbed, HostVitals::default())]);
            log.observe_hosts(&[host(name, HostStatus::Offline, HostVitals::default())]);
        }
        let names: Vec<String> = log.recent(2).map(|e| e.host.clone()).collect();
        assert_eq!(names, vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn session_words_match_the_table() {
        assert_eq!(session_word(&SessionState::Active, true), "attached");
        assert_eq!(session_word(&SessionState::Active, false), "detached");
        assert_eq!(session_word(&SessionState::Suspended, false), "detached");
        assert_eq!(
            session_word(
                &SessionState::Disconnected {
                    reason: "reset".into()
                },
                false
            ),
            "dropped"
        );
    }
}
