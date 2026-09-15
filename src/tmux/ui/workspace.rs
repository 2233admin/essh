use super::super::navigation::{self, View};
use super::*;
use crossterm::event::{MouseButton, MouseEventKind};
use std::{sync::mpsc, time::Instant};

#[derive(Clone)]
enum Click {
    Key(KeyCode),
    Session(String),
    Row(usize),
    Panel(usize),
}
#[derive(Default)]
struct State {
    selected: usize,
    query: String,
    filtering: bool,
    table: TableState,
    focus: usize,
    nav_focus: Option<usize>,
    scroll: [u16; 6],
    help_scroll: u16,
    memory_sort: bool,
    interface: usize,
    hits: Vec<(Rect, Click)>,
    message: String,
}
const PANELS: [&str; 6] = ["CPU", "Memory", "Network", "Storage", "Processes", "Peers"];

pub fn run(mut config: AppConfig, initial: View, popup: bool, new_session: bool) -> Result<()> {
    let mut screen = Screen::new()?;
    let mut view = initial;
    let mut back = Vec::new();
    let mut state = State::default();
    let mut host_list = super::super::hosts(&config);
    let mut ws = windows().unwrap_or_default();
    let mut session = navigation::current()?;
    let mut collector: Option<metrics::HealthCollector> = None;
    let mut collector_id = String::new();
    let (mut refreshed, mut next_probe) = (Instant::now(), Instant::now());
    let (tx, rx) = mpsc::channel::<(String, Option<f64>)>();
    let mut probing = false;
    let mut pending: usize = 0;
    let mut probes: HashMap<String, Probe> = HashMap::new();
    let mut last_click: Option<(usize, Instant)> = None;
    if view == View::Health {
        if let Some(w) = &session {
            select(&w.target)?;
        } else {
            view = View::Hosts;
            state.message = "Connect to a host to view health".into();
        }
    }
    loop {
        if refreshed.elapsed() > Duration::from_millis(500) {
            ws = windows().unwrap_or_default();
            refreshed = Instant::now();
            if view == View::Health {
                if let Some(active) = ws.iter().find(|w| w.active) {
                    session = Some(active.clone());
                }
                if session
                    .as_ref()
                    .is_some_and(|w| !ws.iter().any(|v| v.id == w.id))
                {
                    session = None;
                    view = View::Hosts;
                }
            }
        }
        if view == View::Health {
            if let Some(w) = &session {
                if collector_id != w.id {
                    drop(collector.take());
                    collector = metrics::HealthCollector::new(w).ok();
                    collector_id = w.id.clone();
                    state.scroll = [0; 6];
                    state.interface = 0;
                }
                if let Some(c) = &collector {
                    c.touch();
                }
            }
        } else {
            collector = None;
            collector_id.clear();
        }
        // Probe off the input thread. Only the main host list starts sweeps.
        while let Ok((name, latency)) = rx.try_recv() {
            pending = pending.saturating_sub(1);
            let p = probes.entry(name.clone()).or_default();
            p.latency = latency;
            p.checked = true;
            if let Some(v) = latency {
                p.history.push(v as u64);
                if p.history.len() > 10 {
                    p.history.remove(0);
                }
            }
            if let Ok(raw) = serde_json::to_vec(&(chrono::Utc::now().timestamp(), latency)) {
                let _ = fs::write(probe_path(&name), raw);
            }
        }
        if probing && pending == 0 {
            probing = false;
            next_probe = Instant::now() + Duration::from_secs(5);
        }
        if !popup && !probing && Instant::now() >= next_probe {
            pending = host_list.len();
            probing = true;
            let list = host_list.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                for chunk in list.chunks(8) {
                    std::thread::scope(|scope| {
                        for h in chunk {
                            let tx = tx.clone();
                            scope.spawn(move || {
                                let _ = tx.send((h.name.clone(), tcp_latency(h)));
                            });
                        }
                    });
                }
            });
        }
        let q = state.query.to_lowercase();
        let shown: Vec<Host> = host_list
            .iter()
            .filter(|h| format!("{} {}", h.name, h.tags).to_lowercase().contains(&q))
            .cloned()
            .collect();
        let filtered: Vec<Window> = ws
            .iter()
            .filter(|w| w.host.to_lowercase().contains(&q))
            .cloned()
            .collect();
        let count = if view == View::Hosts {
            shown.len()
        } else {
            filtered.len()
        };
        state.selected = state.selected.min(count.saturating_sub(1));
        state.table.select((count > 0).then_some(state.selected));
        let snapshot = session.as_ref().and_then(|w| metrics::read(&w.id));
        screen.term.draw(|f|{
            background(f);state.hits.clear();
            let rows=Layout::vertical([Constraint::Length(2),Constraint::Length(2),Constraint::Min(1),Constraint::Length(2)]).split(f.area());
            draw_nav(f,rows[0],&ws,&mut state);
            let title=match view {
                View::Hosts=>format!("Main menu │ {} hosts · {} sessions{}",host_list.len(),ws.len(),if new_session{" · new session"}else{""}),
                View::Sessions=>"Sessions │ Enter switch · n New · x Close session".into(),
                View::Help=>"Help │ Navigation and terminal keys".into(),
                View::Health=>{
                    if let Some(w)=&session {format!("Health │ {} · session {} · {}",w.host,w.index,w.state)}else{"Health │ No session".into()}
                }
            };
            line(f,rows[1],title,CYAN);
            match view {
                View::Hosts=>draw_hosts(f,rows[2],&shown,&ws,&probes,&mut state),
                View::Sessions=>draw_sessions(f,rows[2],&filtered,&mut state),
                View::Help=>draw_help(f,rows[2],&state),
                View::Health=>draw_health(f,rows[2],snapshot.as_ref(),&mut state),
            }
            let footer=if state.filtering {format!("/ {}  Enter finish · Esc clear",state.query)}else if !state.message.is_empty(){state.message.clone()}else{
                match view {
                    View::Hosts=>"↑↓ Select · Enter Connect · a Add · e Edit · / Filter · q Quit essh (keep jobs)".into(),
                    View::Sessions=>"↑↓ Select · Enter Switch · 1–9 Jump · / Filter · n New · x Close · Esc Back".into(),
                    View::Help=>"↑↓ Scroll · F1/Esc Back · Tab navigation · Enter activate".into(),
                    View::Health=>"Tab Panel · ↑↓ Scroll · i Interface · c/m Sort · b Beside terminal · r Reconnect · F2/Esc Terminal".into(),
                }
            };
            f.render_widget(Paragraph::new(footer).style(Style::default().fg(DIM)).wrap(Wrap{trim:false}),rows[3]);
            let actions:Vec<(&str,KeyCode)>=match view {
                View::Hosts=>vec![("Connect",KeyCode::Enter),("Add",KeyCode::Char('a')),("Edit",KeyCode::Char('e')),("Quit essh",KeyCode::Char('q'))],
                View::Sessions=>vec![("Switch",KeyCode::Enter),("New session",KeyCode::Char('n')),("Close session",KeyCode::Char('x')),("Back",KeyCode::Esc)],
                View::Health=>vec![("Terminal",KeyCode::F(2)),("Side strip",KeyCode::Char('b')),("Reconnect",KeyCode::Char('r'))],
                View::Help=>vec![("Back",KeyCode::Esc)],
            };
            if rows[3].height>1 && !state.filtering && state.message.is_empty(){
                let y=rows[3].bottom()-1;f.render_widget(Block::default().style(Style::default().bg(BG)),Rect::new(rows[3].x,y,rows[3].width,1));
                let mut x=rows[3].x;
                for (label,key) in actions {let text=format!(" [{label}] ");let width=text.len() as u16;if x+width>rows[3].right(){break;}let r=Rect::new(x,y,width,1);line(f,r,text,CYAN);state.hits.push((r,Click::Key(key)));x+=width;}
            }
        })?;
        if !event::poll(Duration::from_millis(150))? {
            continue;
        }
        let event = event::read()?;
        let mut code = None;
        let mut chosen = None;
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => code = Some(k.code),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    if let Some((_, Click::Panel(n))) = state.hits.iter().rev().find(|(r, hit)| {
                        matches!(hit, Click::Panel(_)) && r.contains(Position::new(m.column, m.row))
                    }) {
                        state.focus = *n;
                    }
                    code = Some(if m.kind == MouseEventKind::ScrollDown {
                        KeyCode::Down
                    } else {
                        KeyCode::Up
                    });
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((_, hit)) = state
                        .hits
                        .iter()
                        .rev()
                        .find(|(r, _)| r.contains(Position::new(m.column, m.row)))
                        .cloned()
                    {
                        match hit {
                            Click::Key(k) => code = Some(k),
                            Click::Session(id) => chosen = ws.iter().find(|w| w.id == id).cloned(),
                            Click::Panel(n) => state.focus = n,
                            Click::Row(n) => {
                                state.selected = n;
                                if last_click.as_ref().is_some_and(|(i, t)| {
                                    *i == n && t.elapsed() < Duration::from_millis(450)
                                }) {
                                    code = Some(KeyCode::Enter);
                                }
                                last_click = Some((n, Instant::now()));
                            }
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        }
        if let Some(w) = chosen {
            select(&w.target)?;
            session = Some(w);
            if view != View::Health {
                if back.contains(&View::Health) {
                    view = View::Health;
                    back.clear();
                } else {
                    if popup {
                        return Ok(());
                    }
                    view = View::Hosts;
                }
            }
            continue;
        }
        let Some(key) = code else {
            continue;
        };
        state.message.clear();
        let key = if key == KeyCode::Enter && state.nav_focus.is_some() {
            KeyCode::F(state.nav_focus.take().unwrap() as u8 + 1)
        } else {
            key
        };
        if let Some(b) = navigation::binding(key) {
            last_click = None;
            state.nav_focus = None;
            state.filtering = false;
            state.query.clear();
            match b.number {
                1 => {
                    if view == View::Help {
                        if let Some(v) = back.pop() {
                            view = v;
                        } else if popup {
                            return Ok(());
                        } else {
                            view = View::Hosts;
                        }
                    } else {
                        back.push(view);
                        view = View::Help;
                    }
                }
                2 => {
                    if view == View::Health {
                        if popup {
                            return Ok(());
                        }
                        view = View::Hosts;
                        if let Some(w) = &session {
                            select(&w.target)?;
                        }
                    } else {
                        session = navigation::current()?;
                        if let Some(w) = &session {
                            select(&w.target)?;
                            view = View::Health;
                        } else {
                            state.message = "Connect to a host to view health".into();
                        }
                    }
                }
                3 => {
                    if view == View::Sessions {
                        if let Some(v) = back.pop() {
                            view = v;
                        } else if popup {
                            return Ok(());
                        } else {
                            view = View::Hosts;
                        }
                    } else {
                        back.push(view);
                        view = View::Sessions;
                        state.selected = ws.iter().position(|w| w.active).unwrap_or(0);
                    }
                }
                4 => {
                    navigation::main_menu()?;
                    if popup {
                        return Ok(());
                    }
                    view = View::Hosts;
                    back.clear();
                }
                5 | 6 => {
                    session = navigation::cycle(if b.number == 5 { -1 } else { 1 })?;
                    if view != View::Health {
                        if back.contains(&View::Health) {
                            view = View::Health;
                            back.clear();
                        } else {
                            if popup {
                                return Ok(());
                            }
                            view = View::Hosts;
                        }
                    }
                }
                _ => (),
            }
            continue;
        }
        if state.filtering {
            match key {
                KeyCode::Esc => {
                    state.query.clear();
                    state.filtering = false;
                }
                KeyCode::Enter => state.filtering = false,
                KeyCode::Backspace => {
                    state.query.pop();
                }
                KeyCode::Char(c) => state.query.push(c),
                _ => (),
            }
            state.selected = 0;
            continue;
        }
        match key {
            KeyCode::Esc => {
                if let Some(v) = back.pop() {
                    view = v;
                } else if popup {
                    return Ok(());
                } else if let Some(w) = navigation::current()? {
                    select(&w.target)?;
                    view = View::Hosts;
                }
            }
            KeyCode::Char('?') => {
                back.push(view);
                view = View::Help;
            }
            KeyCode::Char('/') if matches!(view, View::Hosts | View::Sessions) => {
                state.filtering = true
            }
            KeyCode::Tab => {
                if view == View::Health || view == View::Help {
                    state.focus = (state.focus + 1) % 6;
                } else {
                    state.nav_focus = Some(state.nav_focus.map(|n| (n + 1) % 6).unwrap_or(0));
                }
            }
            KeyCode::BackTab => {
                if view == View::Health || view == View::Help {
                    state.focus = (state.focus + 5) % 6;
                } else {
                    state.nav_focus = Some(state.nav_focus.map(|n| (n + 5) % 6).unwrap_or(5));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => match view {
                View::Help => state.help_scroll = state.help_scroll.saturating_add(1).min(65),
                View::Health => {
                    state.scroll[state.focus] = state.scroll[state.focus].saturating_add(1).min(100)
                }
                _ => state.selected = (state.selected + 1).min(count.saturating_sub(1)),
            },
            KeyCode::Up | KeyCode::Char('k') => match view {
                View::Help => state.help_scroll = state.help_scroll.saturating_sub(1),
                View::Health => {
                    state.scroll[state.focus] = state.scroll[state.focus].saturating_sub(1)
                }
                _ => state.selected = state.selected.saturating_sub(1),
            },
            KeyCode::Enter if view == View::Hosts => {
                if let Some(h) = shown.get(state.selected) {
                    match open(h, new_session) {
                        Ok(()) => {
                            if popup {
                                return Ok(());
                            }
                        }
                        Err(e) => state.message = e.to_string(),
                    }
                }
            }
            KeyCode::Enter if view == View::Sessions => {
                if let Some(w) = filtered.get(state.selected) {
                    select(&w.target)?;
                    session = Some(w.clone());
                    if back.last() == Some(&View::Health) {
                        back.pop();
                        view = View::Health;
                    } else if popup {
                        return Ok(());
                    } else {
                        view = View::Hosts;
                    }
                }
            }
            KeyCode::Char(c @ '1'..='9') if view == View::Sessions => {
                if let Some(w) = ws.iter().find(|w| w.index == c.to_string()) {
                    select(&w.target)?;
                    session = Some(w.clone());
                    if back.last() == Some(&View::Health) {
                        back.pop();
                        view = View::Health;
                    } else if popup {
                        return Ok(());
                    } else {
                        view = View::Hosts;
                    }
                }
            }
            KeyCode::Char('n') if view == View::Sessions => {
                drop(screen);
                return run(config, View::Hosts, popup, true);
            }
            KeyCode::Char('x') if view == View::Sessions => {
                if let Some(w) = filtered.get(state.selected) {
                    drop(screen);
                    state.message = kill(&w.target)
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    screen = Screen::new()?;
                }
            }
            KeyCode::Char('q') if view == View::Hosts => {
                if popup {
                    return Ok(());
                }
                call(&["detach-client"])?;
            }
            KeyCode::Char(c @ ('a' | 'e')) if view == View::Hosts => {
                let original = if c == 'e' {
                    shown.get(state.selected)
                } else {
                    None
                };
                drop(screen);
                let result = host_form(&mut config, original);
                screen = Screen::new()?;
                state.message = result.err().map(|e| e.to_string()).unwrap_or_default();
                host_list = super::super::hosts(&config);
            }
            KeyCode::Char('b') if view == View::Health => {
                if let Some(w) = &session {
                    toggle(&w.target)?;
                    if popup {
                        return Ok(());
                    }
                    view = View::Hosts;
                }
            }
            KeyCode::Char('r') if view == View::Health => {
                if let Some(w) = &session {
                    let s = saved(&w.id)?;
                    open(&s.host, false)?;
                }
            }
            KeyCode::Char('i') if view == View::Health => {
                state.interface = state.interface.saturating_add(1)
            }
            KeyCode::Char('c') if view == View::Health => state.memory_sort = false,
            KeyCode::Char('m') if view == View::Health => state.memory_sort = true,
            KeyCode::Enter if view == View::Help => {
                let b = &navigation::BINDINGS[state.focus];
                match b.number {
                    1 => (),
                    2 => {
                        session = navigation::current()?;
                        view = if session.is_some() {
                            View::Health
                        } else {
                            View::Hosts
                        };
                    }
                    3 => view = View::Sessions,
                    4 => {
                        navigation::main_menu()?;
                        if popup {
                            return Ok(());
                        }
                        view = View::Hosts;
                    }
                    5 | 6 => {
                        session = navigation::cycle(if b.number == 5 { -1 } else { 1 })?;
                        if popup {
                            return Ok(());
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

fn draw_nav(f: &mut Frame, area: Rect, ws: &[Window], s: &mut State) {
    let mut x = area.x;
    for b in navigation::BINDINGS {
        let label = navigation::label(b, area.width);
        let width = (label.len() as u16).min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        f.render_widget(
            Paragraph::new(label).style(Style::default().fg(CYAN).bg(
                if s.nav_focus == Some(b.number as usize - 1) {
                    SELECTED
                } else {
                    BG
                },
            )),
            rect,
        );
        s.hits.push((rect, Click::Key(KeyCode::F(b.number))));
        x += width;
    }
    x = area.x;
    for w in ws {
        let label = format!(
            " {} {}{} ",
            w.index,
            w.host,
            if w.active { " ●" } else { "" }
        );
        let width = (label.chars().count() as u16).min(24);
        if x + width + 12 > area.right() {
            let r = Rect::new(x, area.y + 1, area.right().saturating_sub(x), 1);
            line(f, r, "… F3 Sessions", CYAN);
            s.hits.push((r, Click::Key(KeyCode::F(3))));
            break;
        }
        let r = Rect::new(x, area.y + 1, width, 1);
        f.render_widget(
            Paragraph::new(label).style(
                Style::default()
                    .fg(if w.active { CYAN } else { DIM })
                    .bg(if w.active { SELECTED } else { BG }),
            ),
            r,
        );
        s.hits.push((r, Click::Session(w.id.clone())));
        x += width;
    }
}
fn row_hits(area: Rect, len: usize, s: &mut State) {
    for i in 0..area.height.saturating_sub(2) as usize {
        let n = i + s.table.offset();
        if n < len {
            s.hits.push((
                Rect::new(area.x, area.y + 2 + i as u16, area.width, 1),
                Click::Row(n),
            ));
        }
    }
}
fn draw_hosts(
    f: &mut Frame,
    r: Rect,
    hosts: &[Host],
    ws: &[Window],
    probes: &HashMap<String, Probe>,
    s: &mut State,
) {
    let wide = r.width >= 105;
    let rows = hosts.iter().map(|h| {
        let sessions: Vec<_> = ws.iter().filter(|w| w.host == h.name).collect();
        let connected = sessions.iter().any(|w| w.state == "connected");
        let p = probes.get(&h.name);
        let latency = p
            .and_then(|p| p.latency)
            .or_else(|| cached_latency(&h.name));
        let state = if connected {
            "connected"
        } else if latency.is_some() {
            "reachable"
        } else if h.proxy {
            "via proxy · unknown"
        } else if !sessions.is_empty() {
            "saved · unknown"
        } else {
            "offline"
        };
        let mut cells = vec![h.name.clone()];
        if wide {
            cells.push(format!(
                "{}@{}",
                h.user.as_deref().unwrap_or("—"),
                h.hostname
            ));
        }
        cells.push(if sessions.is_empty() {
            "·".into()
        } else {
            sessions.len().to_string()
        });
        cells.push(
            latency
                .map(|v| format!("{v:.0}ms"))
                .unwrap_or_else(|| "—".into()),
        );
        cells.push(state.into());
        Row::new(cells).style(Style::default().fg(if connected {
            GREEN
        } else if !sessions.is_empty() && latency.is_none() {
            RED
        } else {
            TEXT
        }))
    });
    let (mut headers, mut widths) = (vec!["HOST"], vec![Constraint::Length(18)]);
    if wide {
        headers.push("USER@ADDRESS");
        widths.push(Constraint::Length(24));
    }
    headers.extend(["TMUX", "LATENCY", "STATE"]);
    widths.extend([
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Min(1),
    ]);
    f.render_stateful_widget(
        Table::new(rows, widths)
            .header(
                Row::new(headers)
                    .bottom_margin(1)
                    .style(Style::default().fg(DIM)),
            )
            .column_spacing(2)
            .highlight_symbol(Span::styled("▌", Style::default().fg(CYAN)))
            .row_highlight_style(Style::default().bg(SELECTED).bold()),
        r,
        &mut s.table,
    );
    row_hits(r, hosts.len(), s);
}
fn draw_sessions(f: &mut Frame, r: Rect, ws: &[Window], s: &mut State) {
    let rows = ws.iter().map(|w| {
        Row::new([
            w.index.clone(),
            w.host.clone(),
            metrics::read(&w.id)
                .and_then(|m| m.running)
                .unwrap_or_default(),
            w.state.clone(),
        ])
    });
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(18),
                Constraint::Length(14),
                Constraint::Min(1),
            ],
        )
        .header(Row::new(["#", "HOST", "COMMAND", "STATE"]).bottom_margin(1))
        .column_spacing(1)
        .highlight_symbol("▌")
        .row_highlight_style(Style::default().bg(SELECTED).fg(CYAN)),
        r,
        &mut s.table,
    );
    row_hits(r, ws.len(), s);
}
fn draw_help(f: &mut Frame, r: Rect, s: &State) {
    let mut text = String::from("GETTING AROUND — no prefix needed\n");
    for (i, b) in navigation::BINDINGS.iter().enumerate() {
        text.push_str(&format!(
            "{} F{} {:10} {}\n",
            if s.focus == i { "▌" } else { " " },
            b.number,
            b.label,
            b.description
        ));
    }
    text.push_str("\nMOUSE\nClick session tabs to switch. Click navigation buttons for views.\nClick rows to select; double-click connects/switches.\nWheel scrolls; click panes to focus; drag dividers to resize.\nUse your terminal's selection override (Shift-drag in Ghostty) to select text.\n\nSESSIONS\nEnter connects or reconnects. F3 then n creates another session.\nF4 Main returns to hosts; q there quits essh and keeps jobs running.\nx in Sessions closes a remote session after explicit confirmation.\nSaved/disconnected does not guarantee remote jobs are alive.\n\nHEALTH\nF2 full dashboard; F2/Esc returns to the terminal.\nF5/F6 or session tabs change the dashboard's host.\nTab selects a panel; arrows/wheel scroll it. i cycles interfaces.\nc/m sorts processes by CPU/memory. b shows the side strip.\n— means unavailable; stale samples show their age.\nNetwork and disk rates are bytes/s; process CPU is ps lifetime average.\n\nSPLITS ON THE REMOTE HOST\nCtrl+B, release, then % splits side by side; \" splits top/bottom.\nCtrl+B then arrow changes remote pane. Local Ctrl+A splits open local shells.\n\nADVANCED TMUX\nLocal prefix Ctrl+A; remote prefix Ctrl+B.\nCtrl+A then 1–9 session; s switcher; d main; q detach.\nCtrl+A, release, then lowercase m toggles the side strip.\nCtrl+A then v sends the next key directly to the remote terminal,\nincluding reserved F1–F6. Ctrl+A twice sends a literal Ctrl+A.\nEsc and Ctrl+C in the terminal still go to your remote program.\n");
    f.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((s.help_scroll, 0)),
        r,
    );
}
mod health;
use health::draw_health;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn navigation_buttons_fit_and_match_the_registry_on_small_terminals() {
        for width in [40, 60, 80, 120] {
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            let mut s = State::default();
            terminal
                .draw(|f| draw_nav(f, Rect::new(0, 0, width, 2), &[], &mut s))
                .unwrap();
            assert_eq!(s.hits.len(), 6);
            for (rect, _) in &s.hits {
                assert!(rect.right() <= width && rect.width > 0);
            }
            for pair in s.hits.windows(2) {
                assert!(pair[0].0.right() <= pair[1].0.x);
            }
        }
    }
    #[test]
    fn health_panels_remain_accessible_at_all_supported_sizes() {
        let m = metrics::Snapshot {
            at: chrono::Utc::now().timestamp(),
            cpu: Some(23.0),
            mem: Some(45.0),
            ..Default::default()
        };
        for (width, height) in [(60, 20), (80, 24), (120, 40), (192, 46)] {
            for (focus, panel_name) in PANELS.iter().enumerate() {
                let mut s = State {
                    focus,
                    ..Default::default()
                };
                let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
                term.draw(|f| draw_health(f, f.area(), Some(&m), &mut s))
                    .unwrap();
                let buf = term.backend().buffer();
                let text = buf.content.iter().map(|c| c.symbol()).collect::<String>();
                assert!(
                    text.contains(panel_name),
                    "missing {} at {width}x{height}",
                    panel_name
                );
                for (r, _) in s.hits {
                    assert!(r.right() <= width && r.bottom() <= height);
                }
            }
        }
    }
}
