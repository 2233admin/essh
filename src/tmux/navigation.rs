use super::*;
use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum View {
    Hosts,
    Health,
    Sessions,
    Help,
}

pub struct Binding {
    pub number: u8,
    pub label: &'static str,
    pub command: &'static str,
    pub description: &'static str,
}
pub const BINDINGS: &[Binding] = &[
    Binding {
        number: 1,
        label: "Help",
        command: "help",
        description: "Help; F1 or Esc returns",
    },
    Binding {
        number: 2,
        label: "Health",
        command: "health",
        description: "Full health dashboard; F2 or Esc returns to terminal",
    },
    Binding {
        number: 3,
        label: "Sessions",
        command: "sessions",
        description: "Choose, reconnect or create a session",
    },
    Binding {
        number: 4,
        label: "Main",
        command: "hosts",
        description: "Main menu; keep sessions running",
    },
    Binding {
        number: 5,
        label: "Previous",
        command: "previous",
        description: "Previous SSH session (wraps)",
    },
    Binding {
        number: 6,
        label: "Next",
        command: "next",
        description: "Next SSH session (wraps)",
    },
];
pub fn binding(key: KeyCode) -> Option<&'static Binding> {
    if let KeyCode::F(n) = key {
        BINDINGS.iter().find(|b| b.number == n)
    } else {
        None
    }
}
pub fn label(b: &Binding, width: u16) -> String {
    if width < 54 {
        format!(" F{} ", b.number)
    } else if width < 76 {
        format!(
            " F{} {} ",
            b.number,
            ["?", "Health", "List", "Main", "Prev", "Next"][b.number as usize - 1]
        )
    } else {
        format!(" F{} {} ", b.number, b.label)
    }
}
pub fn configure() -> Result<()> {
    call(&["set-option", "-g", "mouse", "on"])?;
    call(&["set-option", "-g", "status", "2"])?;
    let bar = |width| {
        BINDINGS
            .iter()
            .map(|b| format!("#[range=user|{}]{}#[norange]", b.command, label(b, width)))
            .collect::<String>()
    };
    let bar = format!(
        "#[fg=#4fd1c5]#{{?#{{e|<:#{{client_width}},76}},{},{}}}",
        bar(60),
        bar(120)
    );
    call(&["set-option", "-g", "status-format[1]", &bar])?;
    for b in BINDINGS {
        let key = format!("F{}", b.number);
        if b.number <= 3 {
            call(&[
                "bind-key",
                "-n",
                &key,
                "display-popup",
                "-E",
                "-w",
                "100%",
                "-h",
                "100%",
                "-B",
                &helper(&["view", b.command])?,
            ])?;
        } else {
            call(&[
                "bind-key",
                "-n",
                &key,
                "run-shell",
                "-b",
                &helper(&["navigate", b.command])?,
            ])?;
        }
    }
    // Retain native window-tab targeting and use named ranges for our controls.
    let click = helper(&["navigate", "#{mouse_status_range}"])?;
    call(&[
        "bind-key",
        "-n",
        "MouseDown1Status",
        "if-shell",
        "-F",
        "#{==:#{mouse_status_range},window}",
        "select-window -t =",
        &format!("run-shell -b {}", quote(&click)),
    ])?;
    call(&["bind-key", "v", "switch-client", "-T", "essh-pass"])?;
    call(&["bind-key", "-T", "essh-pass", "Any", "send-keys"])?;
    Ok(())
}
pub fn current() -> Result<Option<Window>> {
    let ws = windows()?;
    let last = call(&["show-options", "-gv", "@essh_last"]).unwrap_or_default();
    Ok(ws
        .iter()
        .find(|w| w.active)
        .or_else(|| ws.iter().find(|w| w.id == last))
        .or(ws.first())
        .cloned())
}
pub fn main_menu() -> Result<()> {
    if let Some(w) = current()? {
        call(&["set-option", "-g", "@essh_last", &w.id])?;
    }
    select("essh:0")
}
pub fn cycle(direction: i32) -> Result<Option<Window>> {
    let ws = windows()?;
    if ws.is_empty() {
        call(&["display-message", "No sessions — choose a host to connect"])?;
        return Ok(None);
    }
    let current = current()?;
    let i = ws
        .iter()
        .position(|w| Some(&w.id) == current.as_ref().map(|w| &w.id))
        .unwrap_or(0);
    let next = (i as i32 + direction).rem_euclid(ws.len() as i32) as usize;
    select(&ws[next].target)?;
    call(&["set-option", "-g", "@essh_last", &ws[next].id])?;
    Ok(Some(ws[next].clone()))
}
pub fn run(command: &str) -> Result<()> {
    match command {
        "hosts" => main_menu(),
        "previous" => {
            cycle(-1)?;
            Ok(())
        }
        "next" => {
            cycle(1)?;
            Ok(())
        }
        "health" | "sessions" | "help" => {
            // Mouse actions run outside a popup; keyboard popup helpers set a
            // marker through the separate `view` command.
            call(&[
                "display-popup",
                "-E",
                "-w",
                "100%",
                "-h",
                "100%",
                "-B",
                &helper(&["view", command])?,
            ])?;
            Ok(())
        }
        _ => Ok(()),
    }
}
