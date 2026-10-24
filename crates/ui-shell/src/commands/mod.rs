//! Shell command parsing, execution against the Boot Graph, and rendering.
extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graph::{BootEntry, BootGraph, EntryId, Health};

/// A parsed command. Cohesive closed set (SRP): each is one shell verb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Disks,
    List,
    Inspect(String),
    Health,
    Verify(String),
    Boot(String),
    Default(String),
    Log,
    Reboot,
    Unknown(String),
}

/// Something the shell asks the engine to perform (the shell itself does no I/O).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellAction {
    None,
    BootEntry(EntryId),
    SetDefault(EntryId),
    VerifyEntry(EntryId),
    ShowLog,
    Reboot,
}

/// Parse a command line. Total — unknown input becomes `Command::Unknown`.
pub fn parse(line: &str) -> Command {
    let line = line.trim();
    let mut it = line.splitn(2, char::is_whitespace);
    let verb = it.next().unwrap_or("").to_ascii_lowercase();
    let arg = it.next().unwrap_or("").trim().to_string();
    match verb.as_str() {
        "" | "help" | "?" => Command::Help,
        "disks" => Command::Disks,
        "list" | "ls" | "os" => Command::List,
        "inspect" | "show" => Command::Inspect(arg),
        "health" => Command::Health,
        "verify" => Command::Verify(arg),
        "boot" | "start" => Command::Boot(arg),
        "default" => Command::Default(arg),
        "log" => Command::Log,
        "reboot" | "reset" => Command::Reboot,
        other => Command::Unknown(other.to_string()),
    }
}

/// Execute a command against the graph, producing text lines plus an action for
/// the engine. Pure: queries read the graph; effects are returned, not performed.
pub fn render(cmd: &Command, graph: &BootGraph) -> (Vec<String>, ShellAction) {
    match cmd {
        Command::Help => (help_text(), ShellAction::None),
        Command::Disks => (disk_lines(graph), ShellAction::None),
        Command::List => (list_lines(graph), ShellAction::None),
        Command::Health => (health_lines(graph), ShellAction::None),
        Command::Inspect(sel) => (inspect_lines(graph, sel), ShellAction::None),
        Command::Verify(sel) => match resolve(graph, sel) {
            Some(e) => (
                alloc::vec![format!("verifying {}...", e.title)],
                ShellAction::VerifyEntry(e.id.clone()),
            ),
            None => (not_found(sel), ShellAction::None),
        },
        Command::Boot(sel) => match resolve(graph, sel) {
            Some(e) if e.bootable() => (
                alloc::vec![format!("booting {}...", e.title)],
                ShellAction::BootEntry(e.id.clone()),
            ),
            Some(e) => (
                alloc::vec![format!(
                    "{} is not bootable ({})",
                    e.title,
                    health_word(&e.health)
                )],
                ShellAction::None,
            ),
            None => (not_found(sel), ShellAction::None),
        },
        Command::Default(sel) => match resolve(graph, sel) {
            Some(e) => (
                alloc::vec![format!("default set to {}", e.title)],
                ShellAction::SetDefault(e.id.clone()),
            ),
            None => (not_found(sel), ShellAction::None),
        },
        Command::Log => (
            alloc::vec!["(boot log follows)".to_string()],
            ShellAction::ShowLog,
        ),
        Command::Reboot => (alloc::vec!["rebooting...".to_string()], ShellAction::Reboot),
        Command::Unknown(v) => (
            alloc::vec![format!("unknown command: {v} (try 'help')")],
            ShellAction::None,
        ),
    }
}

fn help_text() -> Vec<String> {
    [
        "Commands:",
        "  disks              list detected disks",
        "  list               list operating systems and entries",
        "  inspect <id|#>     show details for one entry",
        "  health             health summary of all entries",
        "  verify <id|#>      re-check an entry's files",
        "  boot <id|#>        boot an entry now",
        "  default <id|#>     set the default entry",
        "  log                show the boot history log",
        "  reboot             restart the machine",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn disk_lines(graph: &BootGraph) -> Vec<String> {
    let mut out = Vec::new();
    for d in &graph.disks {
        out.push(format!(
            "disk {}: {} ({} OS)",
            d.id.0,
            d.label,
            d.systems.len()
        ));
    }
    if out.is_empty() {
        out.push("no disks".to_string());
    }
    out
}

fn list_lines(graph: &BootGraph) -> Vec<String> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for d in &graph.disks {
        for os in &d.systems {
            out.push(format!("[{}] {}", os_name(os.kind), os.name));
            for e in &os.entries {
                out.push(format!(
                    "  {:>2}. {}  {}",
                    idx,
                    e.title,
                    health_word(&e.health)
                ));
                idx += 1;
            }
        }
    }
    if out.is_empty() {
        out.push("no boot entries found".to_string());
    }
    out
}

fn health_lines(graph: &BootGraph) -> Vec<String> {
    let mut out = Vec::new();
    for e in graph.entries() {
        out.push(format!("{:<28} {}", e.title, health_detail(&e.health)));
    }
    if out.is_empty() {
        out.push("no entries".to_string());
    }
    out
}

fn inspect_lines(graph: &BootGraph, sel: &str) -> Vec<String> {
    match resolve(graph, sel) {
        None => not_found(sel),
        Some(e) => {
            let mut out = alloc::vec![
                format!("title:   {}", e.title),
                format!("id:      {}", e.id.as_str()),
                format!("method:  {:?}", e.method),
                format!("health:  {}", health_detail(&e.health)),
            ];
            if let Some(l) = &e.loader {
                out.push(format!("loader:  {}", l.0));
            }
            if let Some(k) = &e.kernel {
                out.push(format!("kernel:  {}", k.0));
            }
            if let Some(i) = &e.initrd {
                out.push(format!("initrd:  {}", i.0));
            }
            if let Some(c) = &e.cmdline {
                out.push(format!("cmdline: {}", c));
            }
            out
        }
    }
}

/// Resolve a selector that is either a numeric index (into the flattened list) or
/// a stable entry-id substring. Deterministic and total.
fn resolve<'a>(graph: &'a BootGraph, sel: &str) -> Option<&'a BootEntry> {
    let sel = sel.trim();
    if sel.is_empty() {
        return None;
    }
    if let Ok(n) = sel.parse::<usize>() {
        return graph.entries().nth(n);
    }
    if let Some(e) = graph.entries().find(|e| e.id.as_str() == sel) {
        return Some(e);
    }
    let mut matches = graph.entries().filter(|e| e.id.as_str().contains(sel));
    let first = matches.next()?;
    if matches.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn not_found(sel: &str) -> Vec<String> {
    alloc::vec![format!("no entry matches '{sel}'")]
}

fn os_name(k: graph::OsKind) -> &'static str {
    use graph::OsKind::*;
    match k {
        Windows => "Windows",
        Linux => "Linux",
        NixOs => "NixOS",
        Bsd => "BSD",
        GenericEfi => "EFI",
        Recovery => "Recovery",
    }
}

fn health_word(h: &Health) -> &'static str {
    match h {
        Health::Healthy => "[ok]",
        Health::Degraded(_) => "[warn]",
        Health::Unbootable(_) => "[fail]",
        Health::Unknown => "[?]",
    }
}

fn health_detail(h: &Health) -> String {
    match h {
        Health::Healthy => "healthy".to_string(),
        Health::Degraded(r) => format!("degraded: {}", r.0),
        Health::Unbootable(r) => format!("unbootable: {}", r.0),
        Health::Unknown => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{
        BootEntry, BootGraph, BootMethod, DiskId, DiskNode, EfiPath, EntryRole, Health, OsKind,
        OsNode, Reason,
    };

    fn sample() -> BootGraph {
        let mut g = BootGraph::new();
        let mut nix = OsNode::new(OsKind::NixOs, "NixOS");
        let mut a = BootEntry::new(
            OsKind::NixOs,
            "NixOS gen 128",
            EntryRole::Generation(128),
            BootMethod::NixGeneration,
        );
        a.health = Health::Healthy;
        a.kernel = Some(EfiPath("\\EFI\\nixos\\bz".to_string()));
        let mut b = BootEntry::new(
            OsKind::NixOs,
            "NixOS gen 127",
            EntryRole::Generation(127),
            BootMethod::NixGeneration,
        );
        b.health = Health::Unbootable(Reason::new("missing kernel"));
        nix.push(a);
        nix.push(b);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "NVMe0".to_string(),
            systems: alloc::vec![nix],
        });
        g.sort_canonical();
        g
    }

    #[test]
    fn parses_verbs_and_args() {
        assert_eq!(parse("list"), Command::List);
        assert_eq!(parse("BOOT 2"), Command::Boot("2".to_string()));
        assert_eq!(
            parse("inspect nixos:gen128"),
            Command::Inspect("nixos:gen128".to_string())
        );
        assert_eq!(parse(""), Command::Help);
        assert!(matches!(parse("frobnicate"), Command::Unknown(_)));
    }

    #[test]
    fn list_shows_entries_with_health_markers() {
        let g = sample();
        let (lines, act) = render(&Command::List, &g);
        assert_eq!(act, ShellAction::None);
        assert!(lines
            .iter()
            .any(|l| l.contains("NixOS gen 128") && l.contains("[ok]")));
        assert!(lines
            .iter()
            .any(|l| l.contains("NixOS gen 127") && l.contains("[fail]")));
    }

    #[test]
    fn boot_healthy_returns_boot_action_by_index() {
        let g = sample();
        let (_lines, act) = render(&Command::Boot("0".to_string()), &g);
        match act {
            ShellAction::BootEntry(id) => assert_eq!(id.as_str(), "nixos:gen128"),
            other => panic!("expected boot action, got {other:?}"),
        }
    }

    #[test]
    fn boot_unbootable_is_refused() {
        let g = sample();
        let (lines, act) = render(&Command::Boot("nixos:gen127".to_string()), &g);
        assert_eq!(act, ShellAction::None);
        assert!(lines[0].contains("not bootable"));
    }

    #[test]
    fn inspect_by_id_shows_details() {
        let g = sample();
        let (lines, _) = render(&Command::Inspect("nixos:gen128".to_string()), &g);
        assert!(lines.iter().any(|l| l.starts_with("kernel:")));
        assert!(lines.iter().any(|l| l.contains("healthy")));
    }

    #[test]
    fn unknown_entry_reports_not_found() {
        let g = sample();
        let (lines, act) = render(&Command::Boot("ghost".to_string()), &g);
        assert_eq!(act, ShellAction::None);
        assert!(lines[0].contains("no entry matches"));
    }
}
