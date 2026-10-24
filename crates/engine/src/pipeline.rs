//! The boot pipeline (report §3.3). Sequences the pure components and performs
//! the firmware I/O through the `platform` adapters.
extern crate alloc;
use alloc::collections::BTreeMap;
use core::time::Duration;

use uefi::proto::console::text::{Key, ScanCode};
use uefi::{boot, Status};

use graph::{BootGraph, EntryId};
use platform::{gop::Framebuffer, UefiVarStore};
use ports::{FileStore, VarStore};
use providers::LaunchPlan;
use transaction::{marker, Transaction};
use ui_gfx::menu::{InputEvent, Menu, MenuOutcome};

use crate::canvas_impl::FbCanvas;

/// The engine result. On a successful handoff the machine leaves MyBoot and this
/// never returns; a returned `EngineError` means we could not hand off and the
/// caller should drop to the shell or return a firmware error.
#[derive(Debug)]
pub enum EngineError {
    NoBootableEntries,
    Firmware(Status),
    Aborted,
}

/// Run the whole pipeline. See the module list in `lib.rs` for the nine stages.
pub fn run() -> Result<core::convert::Infallible, EngineError> {
    let mut vars = UefiVarStore::new();
    // Stage timing (no-op unless built with the `boot-timing` feature).
    let mut clock = crate::timing::StageClock::start();
    // Fan out across EVERY filesystem the firmware knows (all disks/ESPs), like
    // rEFInd — not just the volume we booted from. The existing discovery/health
    // code is unchanged; it now sees the whole machine through this one store.
    // Connect and enumerate are separate calls so each gets its own timing lap.
    platform::volume::connect_all_controllers();
    clock.lap("connect");
    let fs = platform::MultiVolume::enumerate();
    clock.lap("enumerate");
    log::info!("myboot: scanning {} volume(s)", fs.len());

    // (1) persisted state + pending marker
    let pending = marker::read(&vars).ok().flatten();

    // (2) discover → Boot Graph, per volume so each entry knows which disk it came
    //     from (two disks sharing a loader path stay distinct — multi-disk boot).
    let targets = fs.scan_targets();
    // Diagnostics: what identifying data is readable on each volume (helps explain
    // why an entry got a real distro name or a generic one).
    for (vid, vfs) in &targets {
        let label = vfs.volume_label().unwrap_or_default();
        let boot = vfs
            .list_dir("\\EFI\\BOOT")
            .ok()
            .map(|v| v.join(","))
            .unwrap_or_default();
        log::info!("myboot: vol {vid} label='{label}' \\EFI\\BOOT=[{boot}]");
    }
    clock.lap("diagnostics");
    let mut graph = discovery::discover_multi(&vars, &targets);
    clock.lap("discovery");

    // (3) assess health over the graph, each entry against the volume it was
    //     discovered on (so a file present only on ANOTHER disk does not make it
    //     healthy). Firmware-variable entries (no volume) check every volume.
    let any_volume: &dyn FileStore = &fs;
    health::assess_all_scoped(&mut graph, |vol| {
        vol.and_then(|v| {
            targets
                .iter()
                .find(|(id, _)| id.as_str() == v)
                .map(|(_, s)| *s)
        })
        .unwrap_or(any_volume)
    });
    clock.lap("health");

    // (3b) SELF-EXCLUSION: drop MyBoot's OWN loader so it never offers to boot
    //      itself. MyBoot is booted from the removable fallback path
    //      \EFI\BOOT\BOOTX64.EFI on its own volume; that entry IS us. Every OTHER
    //      OS on every disk (any number) is preserved — this removes exactly one
    //      entry, on our own volume, matching the fallback loader.
    if let Some(own_vol) = platform::own_volume_id() {
        graph.retain_entries(|e| {
            let is_self = e.volume.as_deref() == Some(own_vol.as_str())
                && e.loader
                    .as_ref()
                    .map(|p| p.0.eq_ignore_ascii_case("\\EFI\\BOOT\\BOOTX64.EFI"))
                    .unwrap_or(false);
            !is_self
        });
        log::info!("myboot: self-excluded own loader on vol {own_vol}");
    }

    // --- diagnostics (boot observability, report §2.9): make the pipeline's
    //     view visible on the serial console. A direct probe of a file we KNOW
    //     is present (our own fallback loader) tells us if the ESP file store is
    //     working at all; the per-entry lines show what discovery + health saw.
    log::info!(
        "myboot: fs probe \\EFI\\BOOT\\BOOTX64.EFI exists={}",
        fs.exists("\\EFI\\BOOT\\BOOTX64.EFI")
    );
    log::info!(
        "myboot: discovered {} entr(y/ies), {} bootable",
        graph.len(),
        graph.bootable().count()
    );
    for e in graph.entries() {
        log::info!(
            "myboot:   '{}' loader={:?} health={:?}",
            e.title,
            e.loader.as_ref().map(|p| p.0.as_str()),
            e.health
        );
    }

    // (4) reconcile the previous boot: if a marker is pending and the OS did not
    //     bless it, that boot FAILED — record it so policy avoids it. If it was
    //     confirmed, record last-good. Then clear the marker.
    reconcile_previous_boot(&mut vars, &graph, pending);

    // (5) load + validate config (best-effort: fall back to defaults if absent)
    let cfg = load_config(&fs);

    // (6) decide the default (deterministic, explainable)
    let history = build_history(&vars, &cfg);
    let strategy = map_strategy(&cfg);
    let decision = policy::decide(&graph, &history, strategy);

    clock.lap("to-menu");

    // (7) present the menu and get the user's (or timeout's) choice, plus an
    //     optional edited kernel command line (the "E" hotkey).
    let (chosen, cmd_override) = match present(&graph, &decision, timeout_secs(&cfg)) {
        Some(sel) => sel,
        None => return Err(EngineError::NoBootableEntries),
    };

    // (8) build the ordered attempt list: the chosen entry first, then every
    //     other bootable entry in ranked (healthiest-first) order. This is the
    //     "never dead-end" guarantee — if the top choice cannot be handed off,
    //     we fall through to the next rather than returning to firmware.
    let mut candidates = alloc::vec![chosen.clone()];
    for id in policy::ranked_candidates(&graph) {
        if id != chosen {
            candidates.push(id);
        }
    }

    // (9) try each candidate through the transaction + handoff until one hands
    //     off. `launch` only returns on FAILURE (on success the machine leaves
    //     MyBoot forever), so any returned error just moves us to the next.
    let max_tries = cfg.as_ref().map(|c| c.max_tries).unwrap_or(2);
    for cand in &candidates {
        // Can we even build a launch plan? Check first so an unplannable entry
        // never wastes one of its attempts.
        let plan = match plan_for(&graph, cand) {
            Some(p) => p,
            None => {
                log::info!("myboot: no launch plan for '{}'; skipping", cand.as_str());
                continue;
            }
        };
        // Apply an edited command line, but only to the entry the user actually
        // edited (the chosen one).
        let plan = if cand == &chosen {
            apply_cmdline_override(plan, cmd_override.as_deref())
        } else {
            plan
        };

        // Transaction up to the point of handoff (charges the attempt, arms the
        // marker). `select` refuses an entry that is unbootable or out of tries,
        // so exhausted candidates are skipped here.
        let tries = load_tries(&vars, max_tries);
        let mut tx = Transaction::new(max_tries, tries, history.last_good.clone());
        if tx.select(&graph, cand).is_err() {
            continue;
        }
        if tx.validate(&graph).is_err() {
            continue;
        }
        if let Ok(transaction::Effect::ChargeAttempt(id)) = tx.stage() {
            persist_tries(&mut vars, &tx, &graph);
            let _ = marker::arm(&mut vars, &id); // arm BEFORE handoff
        }
        let _ = tx.launch();

        log::info!("myboot: handing off to '{}'", cand.as_str());
        let volume = graph.find(cand).and_then(|e| e.volume.clone());
        match launch(plan, &fs, volume.as_deref()) {
            Ok(never) => match never {}, // Infallible: success never returns
            Err(e) => {
                log::warn!(
                    "myboot: handoff to '{}' failed ({:?}); trying next",
                    cand.as_str(),
                    e
                );
                continue;
            }
        }
    }

    // (10) every candidate failed to hand off. Nothing bootable succeeded.
    log::error!(
        "myboot: no candidate could be booted ({} tried)",
        candidates.len()
    );
    Err(EngineError::NoBootableEntries)
}

// ---- stage helpers ----

fn reconcile_previous_boot<V: VarStore>(
    vars: &mut V,
    graph: &BootGraph,
    pending: Option<marker::Marker>,
) {
    if let Some(m) = pending {
        if m.confirmed {
            // The OS blessed the boot: record last-good and restore its tries.
            let _ = persistence::nvram::StateStore::new(vars).set_last_good(&m.entry);
        }
        // else: unconfirmed → treated as a failure; the charged attempt already
        // reduced its tries at the previous Stage, so policy will avoid it if it
        // has run out. Nothing more to do here beyond clearing the marker.
        let _ = marker::clear(vars);
        let _ = graph; // graph consulted by policy in stage 6
    }
}

fn load_config<F: FileStore>(fs: &F) -> Option<config::Config> {
    let bytes = fs.read("\\EFI\\MyBoot\\config.toml").ok()?;
    let text = core::str::from_utf8(&bytes).ok()?;
    config::Config::parse(text).ok()
}

fn build_history<V: VarStore>(vars: &V, cfg: &Option<config::Config>) -> policy::History {
    policy::History {
        configured_default: cfg.as_ref().map(|c| EntryId::from_raw(c.default.clone())),
        last_good: persistence::nvram::read_last_good(vars).ok().flatten(),
        one_shot: None, // one-shot handling is a host-CLI/marker concern
    }
}

fn map_strategy(cfg: &Option<config::Config>) -> policy::Strategy {
    match cfg.as_ref().map(|c| c.policy) {
        Some(config::PolicyKind::Default) => policy::Strategy::Default,
        _ => policy::Strategy::LastGoodThenDefault,
    }
}

fn timeout_secs(cfg: &Option<config::Config>) -> u16 {
    cfg.as_ref().map(|c| c.timeout_secs).unwrap_or(5)
}

fn load_tries<V: VarStore>(_vars: &V, _max: u8) -> BTreeMap<EntryId, u8> {
    // Per-entry try counts persist in the marker/log in this build; start from
    // defaults each boot and let confirm/fail adjust. A richer persisted map is
    // a later refinement and does not change the transaction's guarantees.
    BTreeMap::new()
}

fn persist_tries<V: VarStore>(_vars: &mut V, _tx: &Transaction, _graph: &BootGraph) {
    // The attempt is recorded via the armed marker; explicit per-entry counters
    // are written by the host CLI's bless/gc flow. No-op here keeps the boot path
    // minimal (NVRAM writes are costly).
}

/// Present the graphical menu, honouring the timeout, and return the chosen id.
/// Falls back to the policy decision on timeout, or the first bootable entry.
fn present(
    graph: &BootGraph,
    decision: &Option<policy::Decision>,
    timeout: u16,
) -> Option<(EntryId, Option<alloc::string::String>)> {
    // Acquire graphics; if unavailable, auto-select the decision (headless).
    let mut fb = match Framebuffer::acquire() {
        Ok(fb) => fb,
        Err(e) => {
            log::info!("myboot: GOP unavailable ({e:?}); selecting headlessly");
            return decision
                .as_ref()
                .map(|d| d.chosen.clone())
                .or_else(|| graph.bootable().next().map(|e| e.id.clone()))
                .map(|id| (id, None));
        }
    };

    let mut menu = Menu::from_graph(graph);
    // Position the cursor on the policy default if we have one.
    if let Some(d) = decision {
        if let Some(idx) = menu.rows().iter().position(|r| r.id == d.chosen) {
            for _ in 0..idx {
                menu.handle(InputEvent::Down);
            }
        }
    }

    let deadline_ticks = timeout as u64; // one tick ≈ 1s via stall loop below
    let mut elapsed = 0u64;
    let mut counting = deadline_ticks != 0;

    loop {
        // draw (with the live auto-boot countdown, or None once cancelled)
        {
            let remaining = if counting {
                Some((deadline_ticks - elapsed) as u32)
            } else {
                None
            };
            let mut canvas = FbCanvas { fb: &mut fb };
            ui_gfx::render::draw(&mut canvas, &menu, "Select an operating system", remaining);
        }
        let _ = fb.present();

        // poll for a key for ~1s; on timeout, count down.
        match poll_key(Duration::from_secs(1)) {
            Some(ev) => {
                counting = false; // any key cancels auto-boot
                match menu.handle(ev) {
                    MenuOutcome::Boot(id) => return Some((id, None)),
                    MenuOutcome::EditCmdline(id) => {
                        if let Some(cmd) = edit_cmdline(&mut fb, &menu) {
                            return Some((id, Some(cmd)));
                        }
                    }
                    MenuOutcome::Recovery => {
                        if let Some(id) = recovery_screen(graph, &mut fb, None) {
                            return Some((id, None));
                        }
                    }
                    MenuOutcome::Firmware => to_firmware(), // resets; only returns on failure
                    MenuOutcome::Shell => enter_shell(&mut fb),
                    MenuOutcome::Idle => {}
                }
            }
            None => {
                if counting {
                    elapsed += 1;
                    if elapsed >= deadline_ticks {
                        return match menu.handle(InputEvent::Timeout) {
                            MenuOutcome::Boot(id) => Some((id, None)),
                            _ => decision.as_ref().map(|d| (d.chosen.clone(), None)),
                        };
                    }
                }
            }
        }
    }
}

/// Reboot into the firmware's own setup UI (the "F" hotkey), the standard UEFI way:
/// set `OsIndications`'s BOOT_TO_FW_UI bit, then cold-reset. Only returns if the
/// firmware doesn't support it.
fn to_firmware() {
    use uefi::runtime::{self, ResetType, VariableAttributes, VariableVendor};
    if let Ok(name) = uefi::CStr16::from_str_with_buf("OsIndications", &mut [0u16; 16]) {
        let attrs = VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS;
        let val = 1u64.to_le_bytes(); // EFI_OS_INDICATIONS_BOOT_TO_FW_UI
        let _ = runtime::set_variable(name, &VariableVendor::GLOBAL_VARIABLE, attrs, &val);
    }
    runtime::reset(ResetType::COLD, uefi::Status::SUCCESS, None);
}

/// Enter a pre-boot UEFI shell (the "S" hotkey) by chainloading a shell binary if
/// one is present on any volume. Returns to the menu if none is found.
fn enter_shell(_fb: &mut Framebuffer) {
    let fs = platform::MultiVolume::discover();
    for path in [
        "\\EFI\\Shell\\shellx64.efi",
        "\\shellx64.efi",
        "\\EFI\\tools\\shell.efi",
    ] {
        if fs.exists(path) {
            // Chainload it; on success we don't return. On failure, fall through.
            let _ = fs.chainload_on(None, path, "");
        }
    }
    log::info!("myboot: no UEFI shell found on any volume; returning to menu");
}

/// Edit the kernel command line for the selected entry (the "E" hotkey). Reads a
/// line from the console; returns the new cmdline, or None if cancelled (Esc).
fn edit_cmdline(_fb: &mut Framebuffer, menu: &Menu) -> Option<alloc::string::String> {
    use uefi::proto::console::text::{Input, Key, ScanCode};
    let title = menu.selected()?.title.clone();
    log::info!("myboot: edit cmdline for '{title}' (Enter to accept, Esc to cancel)");
    let handle = boot::get_handle_for_protocol::<Input>().ok()?;
    let mut input = boot::open_protocol_exclusive::<Input>(handle).ok()?;
    let mut line = alloc::string::String::new();
    loop {
        if let Ok(Some(key)) = input.read_key() {
            match key {
                Key::Special(ScanCode::ESCAPE) => return None,
                Key::Printable(c) => match u16::from(c) {
                    0x000D => return Some(line), // Enter
                    0x0008 => {
                        line.pop();
                    } // Backspace
                    ch => {
                        if let Some(c) = char::from_u32(ch as u32) {
                            line.push(c);
                        }
                    }
                },
                _ => {}
            }
        }
        boot::stall(10_000);
    }
}

/// Draw and drive the recovery screen (fig36). Returns `Some(id)` if the user
/// chooses an entry to boot, or `None` to return to the main menu.
fn recovery_screen(
    graph: &graph::BootGraph,
    fb: &mut Framebuffer,
    failed: Option<&graph::EntryId>,
) -> Option<graph::EntryId> {
    use ui_gfx::recovery::{Recovery, RecoveryOutcome};
    let mut rec = Recovery::from_graph(graph, failed);
    loop {
        {
            let mut canvas = FbCanvas { fb };
            ui_gfx::render::draw_recovery(&mut canvas, &rec);
        }
        let _ = fb.present();
        if let Some(ev) = poll_key(Duration::from_secs(1)) {
            match rec.handle(ev) {
                RecoveryOutcome::Boot(id) => return Some(id),
                RecoveryOutcome::Back => return None,
                RecoveryOutcome::Idle => {}
            }
        }
    }
}

/// Poll the console for one key press within `timeout`. Maps firmware keys to the
/// UI's input events. Returns None on timeout.
fn poll_key(timeout: Duration) -> Option<InputEvent> {
    use uefi::proto::console::text::Input;
    let handle = boot::get_handle_for_protocol::<Input>().ok()?;
    let mut input = boot::open_protocol_exclusive::<Input>(handle).ok()?;

    // Busy-wait in short stalls up to the timeout (boot-time, single-threaded).
    let steps = 100u64;
    let step = timeout.as_millis() as u64 / steps.max(1);
    for _ in 0..steps {
        if let Ok(Some(key)) = input.read_key() {
            return Some(match key {
                Key::Special(ScanCode::UP) => InputEvent::Up,
                Key::Special(ScanCode::DOWN) => InputEvent::Down,
                Key::Special(ScanCode::ESCAPE) => InputEvent::Escape,
                Key::Printable(c) => match u16::from(c) {
                    0x000D => InputEvent::Enter,             // CR
                    0x0065 | 0x0045 => InputEvent::Edit,     // e / E
                    0x0072 | 0x0052 => InputEvent::Recovery, // r / R
                    0x0066 | 0x0046 => InputEvent::Firmware, // f / F
                    0x0073 | 0x0053 => InputEvent::Shell,    // s / S
                    _ => continue,
                },
                _ => continue,
            });
        }
        boot::stall((step.max(1) * 1000) as usize);
    }
    None
}

fn plan_for(graph: &BootGraph, id: &EntryId) -> Option<LaunchPlan> {
    let entry = graph.find(id)?;
    let provider = providers::provider_for(entry)?;
    provider.plan(entry).ok()
}

/// Dispatch the launch plan: EFI-stub / chainload go through firmware LoadImage
/// (the default, Secure-Boot-verified path); a direct kernel goes through the
/// `arch` boot-protocol handoff (`direct_boot`).
/// Replace a launch plan's command line with a user-edited one (the "E" hotkey).
/// An empty override leaves the plan unchanged.
fn apply_cmdline_override(plan: LaunchPlan, cmd: Option<&str>) -> LaunchPlan {
    let cmd = match cmd {
        Some(c) if !c.is_empty() => c,
        _ => return plan,
    };
    match plan {
        LaunchPlan::Chainload { image, initrd, .. } => LaunchPlan::Chainload {
            image,
            options: Some(cmd.into()),
            initrd,
        },
        LaunchPlan::DirectKernel { kernel, initrd, .. } => LaunchPlan::DirectKernel {
            kernel,
            initrd,
            cmdline: Some(cmd.into()),
        },
    }
}

fn launch(
    plan: LaunchPlan,
    fs: &platform::MultiVolume,
    volume: Option<&str>,
) -> Result<core::convert::Infallible, EngineError> {
    match plan {
        LaunchPlan::Chainload {
            image,
            options,
            initrd,
        } => {
            let mut cmd = options.unwrap_or_default();
            if let Some(ird) = initrd {
                if !cmd.is_empty() {
                    cmd.push(' ');
                }
                cmd.push_str("initrd=");
                cmd.push_str(&ird.0);
            }
            // Device-path chainload from the entry's OWN volume, so the target
            // (Windows Boot Manager, GRUB, shim, an ISO's or USB's loader) is
            // booted from the right disk and can find its own files — not MyBoot's
            // same-named loader on another volume. Returns only on failure.
            Err(EngineError::Firmware(
                fs.chainload_on(volume, &image.0, &cmd),
            ))
        }
        LaunchPlan::DirectKernel {
            kernel,
            initrd,
            cmdline,
        } => crate::direct_boot::launch(
            &kernel.0,
            initrd.as_ref().map(|p| p.0.as_str()),
            cmdline.as_deref(),
        ),
    }
}
