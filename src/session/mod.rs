pub mod manager;

use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Connecting,
    Active,
    #[allow(dead_code)]
    Suspended,
    Reconnecting {
        attempt: u32,
        max: u32,
    },
    Disconnected {
        reason: String,
    },
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Connecting => write!(f, "Connecting"),
            SessionState::Active => write!(f, "Active"),
            SessionState::Suspended => write!(f, "Suspended"),
            SessionState::Reconnecting { attempt, max } => {
                write!(f, "Reconnecting ({}/{})", attempt, max)
            }
            SessionState::Disconnected { reason } => write!(f, "Disconnected: {}", reason),
        }
    }
}

/// Virtual terminal backed by vt100 parser — properly interprets ANSI escapes
pub struct VirtualTerminal {
    parser: vt100::Parser,
    last_rows: u16,
    last_cols: u16,
}

impl VirtualTerminal {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
            last_rows: rows,
            last_cols: cols,
        }
    }

    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    /// Resize the virtual terminal. Returns true if the size actually changed.
    ///
    /// Clamped to at least one cell in each axis. A zero-width or zero-height
    /// screen makes `vt100` subtract past zero and panic, taking the whole
    /// app down — and callers legitimately compute degenerate sizes, because
    /// a split pane in a very narrow window really can be one column wide
    /// before the divider is subtracted.
    pub fn resize(&mut self, rows: u16, cols: u16) -> bool {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.last_rows && cols == self.last_cols {
            return false;
        }
        self.parser.screen_mut().set_size(rows, cols);
        self.last_rows = rows;
        self.last_cols = cols;
        true
    }

    /// Get the visible screen contents as lines of (text, style) pairs
    pub fn screen_lines(&self) -> Vec<Vec<StyledCell>> {
        let screen = self.parser.screen();
        let (rows, cols) = (screen.size().0, screen.size().1);
        let mut lines = Vec::with_capacity(rows as usize);

        for row in 0..rows {
            let mut line = Vec::new();
            let mut col = 0;
            while col < cols {
                let cell = screen.cell(row, col).unwrap();
                let ch = cell.contents();
                let fg = convert_color(cell.fgcolor());
                let bg = convert_color(cell.bgcolor());
                let bold = cell.bold();
                let underline = cell.underline();
                let inverse = cell.inverse();

                line.push(StyledCell {
                    text: if ch.is_empty() {
                        " ".to_string()
                    } else {
                        ch.to_string()
                    },
                    fg,
                    bg,
                    bold,
                    underline,
                    inverse,
                });
                col += 1;
            }
            lines.push(line);
        }
        lines
    }

    /// Get cursor position (row, col)
    pub fn cursor_position(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// The last line of the session that has anything on it. SPEC §6.2.
    ///
    /// This is the sessions screen's key column: it tells you what a detached
    /// session is doing without attaching to it, and it is free — the bytes
    /// are already in this parser. The cursor row is preferred when it has
    /// content, because a shell sitting at a prompt has the prompt on the
    /// cursor row and blank rows under it.
    pub fn last_output_line(&self) -> Option<String> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let (cursor_row, _) = screen.cursor_position();
        let row_text = |row: u16| -> String {
            (0..cols)
                .filter_map(|col| screen.cell(row, col).map(|c| c.contents()))
                .collect::<String>()
                .trim_end()
                .to_string()
        };

        let from = cursor_row.min(rows.saturating_sub(1));
        for row in (0..=from).rev() {
            let text = row_text(row);
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct StyledCell {
    pub text: String,
    pub fg: Option<ratatui::style::Color>,
    pub bg: Option<ratatui::style::Color>,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

fn convert_color(color: vt100::Color) -> Option<ratatui::style::Color> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(ratatui::style::Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(ratatui::style::Color::Rgb(r, g, b)),
    }
}

/// How many times to ask a host which pty a session owns before giving up.
///
/// Five, a tick apart. Enough to outlast a shell that is still being forked
/// when we first look; few enough that a host which cannot answer — a
/// restricted shell, a platform with no `ps` — is asked five times rather
/// than every hundred milliseconds forever.
pub const TTY_ATTEMPTS: u8 = 5;

/// Represents a single SSH session
pub struct Session {
    #[allow(dead_code)]
    pub id: String,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub state: SessionState,
    pub terminal: VirtualTerminal,
    pub created_at: Instant,
    pub has_new_output: bool,
    pub jump_host: Option<String>,
    /// When this session last produced output. Drives the IDLE column.
    pub last_output_at: Instant,
    /// The pty this session's shell owns on the remote host, once we have
    /// asked. `None` means we have not asked or could not tell, and the
    /// RUNNING and CPU columns say so rather than guessing.
    pub tty: Option<String>,
    /// How many times the pty lookup has been attempted.
    ///
    /// Not a bool. It was, and a bool cannot tell "we asked and the host said
    /// no" from "we asked half a second too early". A session is in
    /// `runtimes` the moment its shell channel is opened, and the shell
    /// process is not always a child of the connection's sshd yet — so the
    /// first lookup can find no pty at all, and one attempt meant RUNNING and
    /// CPU stayed `—` for the life of that session.
    ///
    /// Bounded, because a host that genuinely cannot answer must not be asked
    /// forever. `TTY_ATTEMPTS` tries, spaced a tick apart.
    pub tty_attempts: u8,
    /// True while a lookup is in flight, so a slow one is not started again
    /// on every tick underneath itself.
    pub tty_pending: bool,
}

impl Session {
    pub fn new(
        id: String,
        label: String,
        hostname: String,
        port: u16,
        username: String,
        _scrollback: usize,
    ) -> Self {
        // Default terminal size, will be resized on first render
        Self {
            id,
            label,
            hostname,
            port,
            username,
            state: SessionState::Connecting,
            terminal: VirtualTerminal::new(24, 80),
            created_at: Instant::now(),
            has_new_output: false,
            jump_host: None,
            last_output_at: Instant::now(),
            tty: None,
            tty_attempts: 0,
            tty_pending: false,
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }

    /// Whether the pty lookup should run again.
    pub fn wants_tty_lookup(&self) -> bool {
        self.tty.is_none() && !self.tty_pending && self.tty_attempts < TTY_ATTEMPTS
    }

    pub fn idle_secs(&self) -> u64 {
        self.last_output_at.elapsed().as_secs()
    }

    /// The session's own state, in the words SPEC §2.2 uses.
    ///
    /// `attached` is the session you are looking at, which the caller knows
    /// and this does not, so it is passed in. Everything else is the
    /// session's own business.
    pub fn state_word(&self, attached: bool) -> &'static str {
        match self.state {
            SessionState::Active if attached => "attached",
            SessionState::Active => "detached",
            SessionState::Suspended => "detached",
            SessionState::Connecting => "opening",
            SessionState::Reconnecting { .. } => "dropped",
            SessionState::Disconnected { .. } => "dropped",
        }
    }

    /// Whether the connection is gone. A dropped session keeps its scrollback
    /// and has no live process tree, so its CPU is `—` rather than a stale
    /// number.
    pub fn is_dropped(&self) -> bool {
        matches!(
            self.state,
            SessionState::Reconnecting { .. } | SessionState::Disconnected { .. }
        )
    }
}

#[cfg(test)]
mod tty_lookup_tests {
    use super::*;

    fn session() -> Session {
        Session::new(
            "id".into(),
            "db-01".into(),
            "127.0.0.14".into(),
            2222,
            "matt".into(),
            1000,
        )
    }

    /// A session is in `runtimes` the moment its shell channel opens, and the
    /// shell process is not always a child of the connection's sshd yet. One
    /// attempt meant a lookup half a second early left RUNNING and CPU at `—`
    /// for the life of that session, with nothing on screen to say why.
    #[test]
    fn a_lookup_that_was_too_early_is_tried_again() {
        let mut s = session();
        assert!(s.wants_tty_lookup(), "a fresh session is never asked");

        // The shell has not appeared yet.
        s.tty_attempts += 1;
        assert!(s.wants_tty_lookup(), "one miss ended the search");

        // And not started again underneath itself while one is in flight: an
        // exec on a busy connection can take seconds, and the tick is 100ms.
        s.tty_pending = true;
        assert!(!s.wants_tty_lookup(), "a lookup was started twice over");
        s.tty_pending = false;

        s.tty_attempts += 1;
        s.tty = Some("pts/0".into());
        assert!(!s.wants_tty_lookup(), "an answered session is asked again");
    }

    /// And a host that genuinely cannot answer is asked a bounded number of
    /// times, not every tick forever.
    #[test]
    fn a_host_that_cannot_answer_is_not_asked_forever() {
        let mut s = session();
        for _ in 0..TTY_ATTEMPTS {
            assert!(s.wants_tty_lookup());
            s.tty_attempts += 1;
        }
        assert!(!s.wants_tty_lookup(), "the lookup never gives up");
    }
}
