use std::io::Read;
use std::path::PathBuf;

use crate::model::Size;
use crate::protocol::{OptionFlags, PaneDirection, PaneListScope, Request, WindowStep};
use crate::server::socket_path;

pub(crate) struct Invocation {
    pub socket: PathBuf,
    pub request: Request,
    pub attach: bool,
}

pub(crate) fn parse(arguments: &[String]) -> Result<Invocation, String> {
    let mut index = 0;
    let mut explicit_socket = None;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-S" => {
                index += 1;
                explicit_socket = Some(argument(arguments, index, "-S")?.into());
                index += 1;
            }
            value if value.starts_with("-S") && value.len() > 2 => {
                explicit_socket = Some(value[2..].into());
                index += 1;
            }
            "-L" => {
                index += 1;
                let label = argument(arguments, index, "-L")?;
                explicit_socket = Some(std::env::temp_dir().join(format!("tm-{label}.sock")));
                index += 1;
            }
            value if value.starts_with('-') => break,
            _ => break,
        }
    }

    let command = arguments.get(index).map(String::as_str);
    let command = match command {
        None => default_new_session(),
        Some("new-session" | "new") => parse_new_session(&arguments[index + 1..])?,
        Some("attach-session" | "attach" | "a") => parse_attach(&arguments[index + 1..])?,
        Some("detach-client" | "detach") => parse_detach_client(&arguments[index + 1..])?,
        Some("switch-client" | "switchc") => parse_switch_client(&arguments[index + 1..])?,
        Some("refresh-client" | "refresh" | "refreshc") => parse_refresh_client(&arguments[index + 1..])?,
        Some("run-shell" | "run") => parse_run_shell(&arguments[index + 1..])?,
        Some("list-sessions" | "ls") => parse_list_sessions(&arguments[index + 1..])?,
        Some("list-clients" | "lsc") => parse_list_clients(&arguments[index + 1..])?,
        Some("has-session" | "has") => parse_has_session(&arguments[index + 1..])?,
        Some("kill-session" | "kill") => parse_kill_session(&arguments[index + 1..])?,
        Some("new-window" | "neww") => parse_new_window(&arguments[index + 1..])?,
        Some("split-window" | "splitw" | "split") => parse_split_window(&arguments[index + 1..])?,
        Some("new-pane" | "newp") => parse_new_pane(&arguments[index + 1..])?,
        Some("resize-pane" | "resizep") => parse_resize_pane(&arguments[index + 1..])?,
        Some("swap-pane" | "swapp") => parse_swap_pane(&arguments[index + 1..])?,
        Some("break-pane" | "breakp") => parse_break_pane(&arguments[index + 1..])?,
        Some("join-pane" | "joinp") => parse_join_pane("join-pane", &arguments[index + 1..])?,
        Some("move-pane" | "movep") => parse_join_pane("move-pane", &arguments[index + 1..])?,
        Some("respawn-pane" | "respawnp") => parse_respawn(&arguments[index + 1..])?,
        Some("respawn-window" | "respawnw") => parse_respawn_window(&arguments[index + 1..])?,
        Some("clear-history" | "clearhist") => parse_clear_history(&arguments[index + 1..])?,
        Some("rotate-window" | "rotatew") => parse_rotate_window(&arguments[index + 1..])?,
        Some("swap-window" | "swapw") => parse_swap_window(&arguments[index + 1..])?,
        Some("move-window" | "movew") => parse_move_window(&arguments[index + 1..])?,
        Some("link-window" | "linkw") => parse_link_window(&arguments[index + 1..])?,
        Some("unlink-window" | "unlinkw") => parse_unlink_window(&arguments[index + 1..])?,
        Some("list-windows" | "lsw") => parse_list_windows(&arguments[index + 1..])?,
        Some("list-panes" | "lsp") => parse_list_panes(&arguments[index + 1..])?,
        Some("select-window" | "selectw") => parse_select_window(&arguments[index + 1..])?,
        Some("select-layout" | "selectl") => parse_select_layout(&arguments[index + 1..])?,
        Some("last-window" | "last") => parse_last_window(&arguments[index + 1..])?,
        Some("next-window" | "next") => InvocationSpec::plain(Request::NextWindow {
            target: parse_session_window_navigation("next-window", &arguments[index + 1..])?,
        }),
        Some("previous-window" | "prev") => InvocationSpec::plain(Request::PreviousWindow {
            target: parse_session_window_navigation("previous-window", &arguments[index + 1..])?,
        }),
        Some("rename-session" | "rename") => {
            let (target, name) = target_and_name("rename-session", &arguments[index + 1..])?;
            InvocationSpec::plain(Request::RenameSession { target, name })
        }
        Some("rename-window" | "renamew") => {
            let (target, name) = target_and_name("rename-window", &arguments[index + 1..])?;
            InvocationSpec::plain(Request::RenameWindow { target, name })
        }
        Some("kill-window" | "killw") => parse_kill_window(&arguments[index + 1..])?,
        Some("select-pane" | "selectp") => parse_select_pane(&arguments[index + 1..])?,
        Some("kill-pane" | "killp") => parse_kill_pane(&arguments[index + 1..])?,
        Some("send-keys" | "send") => parse_send_keys(&arguments[index + 1..])?,
        Some("send-prefix") => parse_send_prefix(&arguments[index + 1..])?,
        Some("capture-pane" | "capturep") => parse_capture_pane(&arguments[index + 1..])?,
        Some("copy-mode") => parse_copy_mode("copy-mode", &arguments[index + 1..], false)?,
        Some("copy-mode-and-page") => {
            parse_copy_mode("copy-mode-and-page", &arguments[index + 1..], true)?
        },
        Some("display-panes") => parse_display_panes(&arguments[index + 1..])?,
        Some("choose-tree" | "choose") => parse_choose_tree(&arguments[index + 1..])?,
        Some("choose-buffer" | "chooseb") => parse_choose_buffer(&arguments[index + 1..])?,
        Some("choose-client" | "choosec") => parse_choose_client(&arguments[index + 1..])?,
        Some("display-message" | "display") => parse_display_message(&arguments[index + 1..])?,
        Some("set-buffer" | "setb") => parse_set_buffer(&arguments[index + 1..])?,
        Some("show-buffer" | "showb") => InvocationSpec::plain(Request::ShowBuffer {
            name: scan_flags("show-buffer", &arguments[index + 1..], "b:", 0)?
                .value('b')
                .map(str::to_owned),
        }),
        Some("list-buffers" | "lsb") => parse_list_buffers(&arguments[index + 1..])?,
        Some("delete-buffer" | "deleteb") => parse_delete_buffer(&arguments[index + 1..])?,
        Some("paste-buffer" | "pasteb" | "paste") => parse_paste_buffer(&arguments[index + 1..])?,
        Some("load-buffer" | "loadb") => parse_load_buffer(&arguments[index + 1..])?,
        Some("save-buffer" | "saveb") => parse_save_buffer(&arguments[index + 1..])?,
        Some("set-window-option" | "setw") => parse_set_window_option(&arguments[index + 1..])?,
        Some("set-option" | "set") => parse_set_option(&arguments[index + 1..])?,
        Some("show-options" | "show") => parse_show_options(&arguments[index + 1..], false)?,
        Some("show-window-options" | "showw") => parse_show_options(&arguments[index + 1..], true)?,
        Some("set-environment" | "setenv") => parse_set_environment(&arguments[index + 1..])?,
        Some("show-environment" | "showenv") => parse_show_environment(&arguments[index + 1..])?,
        Some("pipe-pane" | "pipep") => parse_pipe_pane(&arguments[index + 1..])?,
        Some("kill-server") => {
            scan_flags("kill-server", &arguments[index + 1..], "", 0)?;
            InvocationSpec::plain(Request::KillServer)
        }
        Some("help" | "-h" | "--help") => return Err(usage().to_owned()),
        Some(unknown) => return Err(format!("unknown command: {unknown}\n\n{}", usage())),
    };

    Ok(Invocation {
        socket: socket_path(explicit_socket.as_deref()),
        request: command.request,
        attach: command.attach,
    })
}

struct InvocationSpec {
    request: Request,
    attach: bool,
}

impl InvocationSpec {
    fn plain(request: Request) -> Self {
        Self {
            request,
            attach: false,
        }
    }
}

fn default_new_session() -> InvocationSpec {
    InvocationSpec {
        request: Request::NewSession {
            name: None,
            detached: false,
            attach_existing: false,
            group_target: None,
            format: None,
            window_name: None,
            command: Vec::new(),
            cwd: current_dir(),
            size: Size::new(80, 24),
        },
        attach: true,
    }
}

fn parse_new_session(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("new-session", arguments, "Ac:dEF:n:Ps:t:x:y:", usize::MAX)?;
    let detached = flags.has('d');
    let attach_existing = flags.has('A');
    if attach_existing && flags.value('s').is_none() {
        return Err(unsupported_flag("-A", "new-session"));
    }
    let format = flags
        .has('P')
        .then(|| flags.value('F').unwrap_or("#{session_name}:").to_owned());
    let mut size = Size::new(80, 24);
    if let Some(value) = flags.value('x') {
        size.cols = parse_dimension(value, "-x")?;
    }
    if let Some(value) = flags.value('y') {
        size.rows = parse_dimension(value, "-y")?;
    }
    Ok(InvocationSpec {
        request: Request::NewSession {
            name: flags.value('s').map(str::to_owned),
            detached,
            attach_existing,
            group_target: flags.value('t').map(str::to_owned),
            format,
            window_name: flags.value('n').map(str::to_owned),
            command: flags.positional.to_vec(),
            cwd: flags.value('c').map(str::to_owned).or_else(current_dir),
            size,
        },
        attach: !detached,
    })
}

fn parse_attach(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("attach-session", arguments, "Et:", 0)?;
    Ok(InvocationSpec {
        request: Request::Attach {
            target: flags.value('t').map(str::to_owned),
            size: crate::terminal::size(),
            term: std::env::var("TERM").unwrap_or_default(),
        },
        attach: true,
    })
}

fn parse_dimension(value: &str, flag: &str) -> Result<u16, String> {
    let dimension = value
        .parse::<u16>()
        .map_err(|_| format!("{flag} requires a positive integer"))?;
    if dimension == 0 {
        return Err(format!("{flag} requires a positive integer"));
    }
    Ok(dimension)
}

fn parse_kill_session(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("kill-session", arguments, "at:", 0)?;
    Ok(InvocationSpec::plain(Request::KillSession {
        target: flags.value('t').map(str::to_owned),
        all: flags.has('a'),
    }))
}

fn parse_kill_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("kill-window", arguments, "af:t:", 0)?;
    if flags.has('f') {
        return Err(unsupported_flag("-f", "kill-window"));
    }
    Ok(InvocationSpec::plain(Request::KillWindow {
        target: flags.value('t').map(str::to_owned),
        all: flags.has('a'),
    }))
}

fn parse_new_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("new-window", arguments, "abc:de:EF:kn:PSt:", usize::MAX)?;
    if flags.has('e') {
        return Err(unsupported_flag("-e", "new-window"));
    }
    let detached = flags.has('d');
    let format = flags.has('P').then(|| {
        flags
            .value('F')
            .unwrap_or("#{session_name}:#{window_index}.#{pane_index}")
            .to_owned()
    });
    let command = flags.positional.to_vec();
    let empty = flags.has('E');
    let (command, empty) = if command.len() == 1 && command[0].is_empty() {
        (command, true)
    } else {
        if empty && !command.is_empty() {
            return Err("command cannot be given for empty pane".to_owned());
        }
        (command, empty)
    };
    Ok(InvocationSpec::plain(Request::NewWindow {
        target: flags.value('t').map(str::to_owned),
        name: flags.value('n').map(str::to_owned),
        detached,
        empty,
        force: flags.has('k'),
        format,
        after: flags.has('a'),
        before: flags.has('b'),
        select_existing: flags.has('S'),
        command,
        cwd: flags.value('c').map(str::to_owned),
    }))
}

fn parse_new_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    scan_flags("new-pane", arguments, "", 0)?;
    Err("unsupported command new-pane".to_owned())
}

fn parse_split_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags(
        "split-window",
        arguments,
        "bB:c:de:EfF:hIkl:m:p:PR:s:S:t:T:vWZ",
        usize::MAX,
    )?;
    for (flag, name) in [('B', "-B"), ('e', "-e"), ('f', "-f"), ('I', "-I"), ('k', "-k"), ('m', "-m"), ('R', "-R"), ('s', "-s"), ('S', "-S"), ('T', "-T"), ('W', "-W")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "split-window"));
        }
    }
    if flags.has('Z') && flags.has('d') {
        return Err(unsupported_flag("-Z (with -d)", "split-window"));
    }
    let format = flags.has('P').then(|| {
        flags
            .value('F')
            .unwrap_or("#{session_name}:#{window_index}.#{pane_index}")
            .to_owned()
    });
    let command = flags.positional.to_vec();
    let (command, empty) = if command.len() == 1 && command[0].is_empty() {
        (command, true)
    } else {
        if flags.has('E') && !command.is_empty() {
            return Err("command cannot be given for empty pane".to_owned());
        }
        (command, flags.has('E'))
    };
    Ok(InvocationSpec::plain(Request::SplitWindow {
        target: flags.value('t').map(str::to_owned),
        horizontal: flags.has('h'),
        before: flags.has('b'),
        detached: flags.has('d'),
        empty,
        zoom: flags.has('Z'),
        size: flags.value('l').map(str::to_owned),
        percentage: flags.value('p').map(str::to_owned),
        command,
        cwd: flags.value('c').map(str::to_owned),
        format,
    }))
}

fn parse_list_sessions(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("list-sessions", arguments, "F:", 0)?;
    Ok(InvocationSpec::plain(Request::ListSessions {
        format: flags.value('F').map(str::to_owned),
    }))
}

fn parse_list_clients(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("list-clients", arguments, "F:", 0)?;
    Ok(InvocationSpec::plain(Request::ListClients {
        format: flags.value('F').map(str::to_owned),
    }))
}

fn parse_detach_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("detach-client", arguments, "at:", 0)?;
    Ok(InvocationSpec::plain(Request::DetachClient {
        target: flags.value('t').map(str::to_owned),
        all: flags.has('a'),
    }))
}

fn parse_switch_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("switch-client", arguments, "c:t:", 0)?;
    let session = flags
        .value('t')
        .ok_or_else(|| "switch-client requires a target session".to_owned())?;
    if session == "=" || session.contains([':', '.', '%']) {
        return Err(format!("unsupported target {session} for switch-client"));
    }
    Ok(InvocationSpec::plain(Request::SwitchClient {
        client: flags.value('c').map(str::to_owned),
        session: session.to_owned(),
    }))
}

fn parse_refresh_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("refresh-client", arguments, "t:", 0)?;
    Ok(InvocationSpec::plain(Request::RefreshClient {
        target: flags.value('t').map(str::to_owned),
    }))
}

fn parse_has_session(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("has-session", arguments, "t:", 0)?;
    Ok(InvocationSpec::plain(Request::HasSession {
        target: flags.value('t').unwrap_or_default().to_owned(),
    }))
}

fn parse_run_shell(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("run-shell", arguments, "bd:Ct:Es:c:", usize::MAX)?;
    for (flag, name) in [('C', "-C"), ('E', "-E"), ('c', "-c")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "run-shell"));
        }
    }
    let delay_ms = flags.value('d').map(run_shell_delay).transpose()?;
    let command = (!flags.positional.is_empty()).then(|| flags.positional.join(" "));
    Ok(InvocationSpec::plain(Request::RunShell {
        command,
        background: flags.has('b'),
        target: flags.value('t').map(str::to_owned),
        cwd: current_dir().ok_or_else(|| "cannot read the current directory".to_owned())?,
        delay_ms,
    }))
}

fn run_shell_delay(value: &str) -> Result<u32, String> {
    let seconds = value
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .ok_or_else(|| format!("invalid delay time: {value}"))?;
    u32::try_from((seconds * 1000.0) as u64).map_err(|_| format!("invalid delay time: {value}"))
}

fn parse_select_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("select-window", arguments, "lnpTt:", 0)?;
    if flags.has('T') {
        return Err(unsupported_flag("-T", "select-window"));
    }
    let step = if flags.has('n') {
        WindowStep::Next
    } else if flags.has('p') {
        WindowStep::Previous
    } else if flags.has('l') {
        WindowStep::Last
    } else {
        WindowStep::Exact
    };
    Ok(InvocationSpec::plain(Request::SelectWindow {
        target: flags.value('t').unwrap_or_default().to_owned(),
        step,
    }))
}

fn last_window_request(session: Option<&str>) -> Request {
    let target = match session {
        None => String::new(),
        Some(target) => format!(
            "{}:",
            target.split_once(':').map_or(target, |(session, _)| session)
        ),
    };
    Request::SelectWindow { target, step: WindowStep::Last }
}

fn parse_session_window_navigation(
    command: &str,
    arguments: &[String],
) -> Result<Option<String>, String> {
    let flags = scan_flags(command, arguments, "at:", 0)?;
    if flags.has('a') {
        return Err(unsupported_flag("-a", command));
    }
    Ok(flags.value('t').map(str::to_owned))
}

fn parse_last_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("last-window", arguments, "t:", 0)?;
    Ok(InvocationSpec::plain(last_window_request(flags.value('t'))))
}

fn parse_select_layout(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("select-layout", arguments, "Enopt:", 1)?;
    for (flag, name) in [('E', "-E"), ('n', "-n"), ('o', "-o"), ('p', "-p")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "select-layout"));
        }
    }
    Ok(InvocationSpec::plain(Request::SelectLayout {
        target: flags.value('t').map(str::to_owned),
        layout: flags
            .positional
            .first()
            .cloned()
            .ok_or_else(|| "select-layout requires a layout".to_owned())?,
    }))
}

fn parse_rotate_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("rotate-window", arguments, "Dt:UZ", 0)?;
    if flags.has('Z') {
        return Err(unsupported_flag("-Z", "rotate-window"));
    }
    Ok(InvocationSpec::plain(Request::RotateWindow {
        target: flags.value('t').map(str::to_owned),
        up: flags.has('U'),
    }))
}

fn parse_swap_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("swap-window", arguments, "ds:t:", 0)?;
    Ok(InvocationSpec::plain(Request::SwapWindow {
        source: flags.value('s').map(str::to_owned),
        target: flags.value('t').map(str::to_owned),
        detached: flags.has('d'),
    }))
}

fn parse_move_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("move-window", arguments, "abdkrs:t:", 0)?;
    if flags.has('b') {
        return Err(unsupported_flag("-b", "move-window"));
    }
    Ok(InvocationSpec::plain(Request::MoveWindow {
        source: flags.value('s').map(str::to_owned),
        target: flags.value('t').map(str::to_owned),
        after: flags.has('a'),
        detached: flags.has('d'),
        force: flags.has('k'),
        renumber: flags.has('r'),
    }))
}

fn parse_link_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    scan_flags("link-window", arguments, "", 0)?;
    Err("unsupported command link-window".to_owned())
}

fn parse_unlink_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("unlink-window", arguments, "kt:", 0)?;
    Ok(InvocationSpec::plain(Request::UnlinkWindow {
        target: flags.value('t').map(str::to_owned),
        force: flags.has('k'),
    }))
}

fn parse_list_windows(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("list-windows", arguments, "aF:f:O:rt:", 0)?;
    for (flag, name) in [('O', "-O"), ('r', "-r")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "list-windows"));
        }
    }
    Ok(InvocationSpec::plain(Request::ListWindows {
        target: flags.value('t').map(str::to_owned),
        format: flags.value('F').map(str::to_owned),
        filter: flags.value('f').map(str::to_owned),
        all: flags.has('a'),
    }))
}

fn parse_list_panes(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("list-panes", arguments, "aF:f:O:rst:", 0)?;
    for (flag, name) in [('O', "-O"), ('r', "-r")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "list-panes"));
        }
    }
    let scope = if flags.has('a') {
        PaneListScope::Server
    } else if flags.has('s') {
        PaneListScope::Session
    } else {
        PaneListScope::Window
    };
    Ok(InvocationSpec::plain(Request::ListPanes {
        target: flags.value('t').map(str::to_owned),
        format: flags.value('F').map(str::to_owned),
        filter: flags.value('f').map(str::to_owned),
        scope,
    }))
}

fn parse_select_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("select-pane", arguments, "DdegLlMmP:RT:t:UZ", 0)?;
    for (flag, name) in [('g', "-g"), ('P', "-P"), ('Z', "-Z")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "select-pane"));
        }
    }
    let direction = if flags.has('l') {
        PaneDirection::Last
    } else if flags.has('m') || flags.has('M') {
        PaneDirection::Target
    } else if flags.has('L') {
        PaneDirection::Left
    } else if flags.has('R') {
        PaneDirection::Right
    } else if flags.has('U') {
        PaneDirection::Up
    } else if flags.has('D') {
        PaneDirection::Down
    } else {
        PaneDirection::Target
    };
    let marking = !flags.has('l') && (flags.has('m') || flags.has('M'));
    let mark = marking.then(|| !flags.has('M'));
    let enabled = if marking {
        None
    } else if flags.has('e') {
        Some(true)
    } else if flags.has('d') {
        Some(false)
    } else {
        None
    };
    let title = (!marking && !flags.has('l') && enabled.is_none())
        .then(|| flags.value('T').map(str::to_owned))
        .flatten();
    Ok(InvocationSpec::plain(Request::SelectPane {
        target: flags.value('t').map(str::to_owned),
        direction,
        mark,
        title,
        enabled,
    }))
}

fn parse_kill_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("kill-pane", arguments, "af:t:", 0)?;
    if flags.value('f').is_some() && !flags.has('a') {
        return Err("-f only valid with -a".to_owned());
    }
    Ok(InvocationSpec::plain(Request::KillPane {
        target: flags.value('t').map(str::to_owned),
        all: flags.has('a'),
        filter: flags.value('f').map(str::to_owned),
    }))
}

fn parse_clear_history(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("clear-history", arguments, "Ht:", 0)?;
    if flags.has('H') {
        return Err(unsupported_flag("-H", "clear-history"));
    }
    Ok(InvocationSpec::plain(Request::ClearHistory {
        target: flags.value('t').map(str::to_owned),
    }))
}

fn parse_resize_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("resize-pane", arguments, "D::L::MR::Tt:U::x:y:Z", 1)?;
    for (flag, name) in [('M', "-M"), ('T', "-T")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "resize-pane"));
        }
    }
    let adjustment = |flag: char| {
        flags.has(flag).then(|| {
            flags
                .value(flag)
                .or_else(|| flags.positional.first().map(String::as_str))
                .unwrap_or("1")
                .to_owned()
        })
    };
    Ok(InvocationSpec::plain(Request::ResizePane {
        target: flags.value('t').map(str::to_owned),
        width: flags.value('x').map(str::to_owned),
        height: flags.value('y').map(str::to_owned),
        up: adjustment('U'),
        down: adjustment('D'),
        left: adjustment('L'),
        right: adjustment('R'),
        zoom: flags.has('Z'),
    }))
}

fn parse_swap_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("swap-pane", arguments, "dDs:t:UZ", 0)?;
    if flags.has('Z') {
        return Err(unsupported_flag("-Z", "swap-pane"));
    }
    let direction = if flags.has('D') {
        Some(PaneDirection::Next)
    } else if flags.has('U') {
        Some(PaneDirection::Previous)
    } else {
        None
    };
    Ok(InvocationSpec::plain(Request::SwapPane {
        source: flags.value('s').map(str::to_owned),
        target: flags.value('t').map(str::to_owned),
        direction,
        detached: flags.has('d'),
    }))
}

fn parse_break_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("break-pane", arguments, "abdPF:n:s:t:Wx:X:y:Y:", 0)?;
    for (flag, name) in [('a', "-a"), ('b', "-b"), ('W', "-W"), ('x', "-x"), ('X', "-X"), ('y', "-y"), ('Y', "-Y")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "break-pane"));
        }
    }
    let format = flags.has('P').then(|| {
        flags
            .value('F')
            .unwrap_or("#{session_name}:#{window_index}.#{pane_index}")
            .to_owned()
    });
    Ok(InvocationSpec::plain(Request::BreakPane {
        source: flags.value('s').map(str::to_owned),
        target: flags.value('t').map(str::to_owned),
        name: flags.value('n').map(str::to_owned),
        detached: flags.has('d'),
        format,
    }))
}

fn parse_join_pane(command: &str, arguments: &[String]) -> Result<InvocationSpec, String> {
    let accepted = if command == "move-pane" {
        "bdfhvl:s:t:"
    } else {
        "bdfhvp:l:s:t:"
    };
    let flags = scan_flags(command, arguments, accepted, 0)?;
    if flags.has('f') {
        return Err(unsupported_flag("-f", command));
    }
    Ok(InvocationSpec::plain(Request::JoinPane {
        source: flags.value('s').map(str::to_owned),
        target: flags.value('t').map(str::to_owned),
        horizontal: flags.has('h'),
        before: flags.has('b'),
        detached: flags.has('d'),
        size: flags.value('l').map(str::to_owned),
        percentage: flags.value('p').map(str::to_owned),
    }))
}

fn parse_respawn(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("respawn-pane", arguments, "c:e:Ekt:", usize::MAX)?;
    for (flag, name) in [('e', "-e"), ('E', "-E")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "respawn-pane"));
        }
    }
    Ok(InvocationSpec::plain(Request::RespawnPane {
        target: flags.value('t').map(str::to_owned),
        command: flags.positional.to_vec(),
        cwd: flags.value('c').map(str::to_owned),
        kill: flags.has('k'),
        empty: false,
        window: false,
    }))
}

fn parse_respawn_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("respawn-window", arguments, "c:e:Ekt:", usize::MAX)?;
    for (flag, name) in [('e', "-e"), ('E', "-E")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "respawn-window"));
        }
    }
    Ok(InvocationSpec::plain(Request::RespawnPane {
        target: flags.value('t').map(str::to_owned),
        command: flags.positional.to_vec(),
        cwd: flags.value('c').map(str::to_owned),
        kill: flags.has('k'),
        empty: false,
        window: true,
    }))
}

fn parse_send_keys(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("send-keys", arguments, "c:FHKlMN:Rt:X", usize::MAX)?;
    for (flag, name) in [('c', "-c"), ('F', "-F"), ('H', "-H"), ('K', "-K"), ('M', "-M")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "send-keys"));
        }
    }
    let target = flags.value('t').map(str::to_owned);
    let literal = flags.has('l');
    let reset = flags.has('R');
    let repeat = match flags.value('N') {
        Some(count) => repeat_count(count)?,
        None => 1,
    };
    let mut positional = flags.positional.iter().cloned();
    let copy_action = flags
        .has('X')
        .then(|| positional.next().unwrap_or_default());
    let mut keys = positional.collect::<Vec<_>>();
    let mut target = target;
    let mut copy_no_paste = false;
    let mut copy_no_clipboard = false;
    if copy_action.is_some() {
        let command_flags = scan_flags("send-keys", &keys, "CPt:", usize::MAX)?;
        copy_no_paste = command_flags.has('P');
        copy_no_clipboard = command_flags.has('C');
        if let Some(command_target) = command_flags.value('t') {
            target = Some(command_target.to_owned());
        }
        keys = command_flags.positional.to_vec();
    }
    if let Some(mut action) = copy_action {
        // Keep copy-mode flags in the action name and use record separators
        // for the two optional copy-pipe arguments. The request protocol
        // already length-prefixes strings, so these control bytes are local
        // framing rather than shell-visible syntax.
        let argument = if matches!(
            action.as_str(),
            "copy-pipe-no-clear"
                | "copy-pipe"
                | "copy-pipe-and-cancel"
                | "copy-pipe-end-of-line"
                | "copy-pipe-end-of-line-and-cancel"
                | "copy-pipe-line"
                | "copy-pipe-line-and-cancel"
                | "pipe-no-clear"
                | "pipe"
                | "pipe-and-cancel"
        ) {
            keys.join("\x1e")
        } else {
            keys.join(" ")
        };
        if copy_no_paste || copy_no_clipboard {
            let mut flags = String::new();
            if copy_no_paste {
                flags.push('P');
            }
            if copy_no_clipboard {
                flags.push('C');
            }
            action = format!("{action}\x1d{flags}");
        }
        let action = if keys.is_empty() {
            action
        } else {
            format!("{action}\0{argument}")
        };
        return Ok(InvocationSpec::plain(Request::CopyModeCommand {
            target,
            action,
            repeat,
        }));
    }
    let bytes = if literal {
        keys.concat().into_bytes()
    } else {
        keys.iter().flat_map(|key| key_bytes(key)).collect()
    };
    let bytes = bytes.repeat(repeat as usize);
    Ok(InvocationSpec::plain(Request::SendKeys {
        target,
        bytes,
        reset,
    }))
}

fn repeat_count(value: &str) -> Result<u32, String> {
    let number = value
        .parse::<i64>()
        .map_err(|_| "repeat count invalid".to_owned())?;
    if number < 1 {
        return Err("repeat count too small".to_owned());
    }
    u32::try_from(number).map_err(|_| "repeat count too large".to_owned())
}

fn parse_copy_mode(
    command: &str,
    arguments: &[String],
    mut page: bool,
) -> Result<InvocationSpec, String> {
    let flags = scan_flags(command, arguments, "dekHMqSs:t:u", 0)?;
    for (flag, name) in [('k', "-k"), ('M', "-M"), ('S', "-S")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, command));
        }
    }
    page |= flags.has('u');
    Ok(InvocationSpec::plain(Request::CopyMode {
        target: flags.value('t').map(str::to_owned),
        source: flags.value('s').map(str::to_owned),
        exit_on_scroll: flags.has('e'),
        hide_position: flags.has('H'),
        page,
        page_down: flags.has('d'),
        reset: flags.has('q'),
    }))
}

fn parse_display_panes(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("display-panes", arguments, "d:kNs:t:Z", 1)?;
    for (flag, name) in [('d', "-d"), ('k', "-k"), ('N', "-N"), ('s', "-s"), ('Z', "-Z")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "display-panes"));
        }
    }
    Ok(InvocationSpec::plain(Request::DisplayPanes {
        target: flags.value('t').map(str::to_owned),
        command: flags
            .positional
            .first()
            .map(|template| crate::config::tokenize(template))
            .unwrap_or_default(),
    }))
}

fn choose_target(
    command: &str,
    arguments: &[String],
    accepted: &str,
) -> Result<Option<String>, String> {
    let flags = scan_flags(command, arguments, accepted, 1)?;
    for flag in accepted.chars().filter(|flag| !matches!(flag, 't' | ':')) {
        if flags.has(flag) {
            return Err(unsupported_flag(&format!("-{flag}"), command));
        }
    }
    if !flags.positional.is_empty() {
        return Err(unsupported_flag("template", command));
    }
    Ok(flags.value('t').map(str::to_owned))
}

fn parse_choose_tree(arguments: &[String]) -> Result<InvocationSpec, String> {
    Ok(InvocationSpec::plain(Request::ChooseTree {
        target: choose_target("choose-tree", arguments, "F:f:GhK:kNO:rst:wyZ")?,
    }))
}

fn parse_choose_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    Ok(InvocationSpec::plain(Request::ChooseBuffer {
        target: choose_target("choose-buffer", arguments, "F:f:K:kNO:rt:yZ")?,
    }))
}

fn parse_choose_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    Ok(InvocationSpec::plain(Request::ChooseClient {
        target: choose_target("choose-client", arguments, "F:f:hiK:kNO:rt:yZ")?,
    }))
}

fn parse_capture_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("capture-pane", arguments, "ab:CeE:FHIJLMNpPqRS:Tt:", 0)?;
    for (flag, name) in [
        ('a', "-a"),
        ('C', "-C"),
        ('F', "-F"),
        ('H', "-H"),
        ('I', "-I"),
        ('L', "-L"),
        ('M', "-M"),
        ('P', "-P"),
        ('q', "-q"),
        ('R', "-R"),
        ('T', "-T"),
    ] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "capture-pane"));
        }
    }
    let print = flags.has('p');
    Ok(InvocationSpec::plain(Request::CapturePane {
        target: flags.value('t').map(str::to_owned),
        start: flags.value('S').and_then(parse_capture_offset),
        end: flags.value('E').and_then(parse_capture_offset),
        escape: flags.has('e'),
        join: flags.has('J'),
        preserve_trailing: flags.has('N'),
        print,
        buffer: flags
            .value('b')
            .filter(|_| !print)
            .map(str::to_owned),
    }))
}

fn parse_capture_offset(value: &str) -> Option<i32> {
    if value == "-" {
        return Some(i32::MIN);
    }
    value
        .parse::<i32>()
        .ok()
        .filter(|offset| *offset <= i32::from(i16::MAX))
}

const DISPLAY_MESSAGE_TEMPLATE: &str = "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index} - (%H:%M %d-%b-%y)";

fn parse_display_message(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("display-message", arguments, "aCc:d:jlINpt:F:v", 1)?;
    for (flag, name) in [
        ('a', "-a"),
        ('C', "-C"),
        ('c', "-c"),
        ('d', "-d"),
        ('I', "-I"),
        ('j', "-j"),
        ('N', "-N"),
        ('v', "-v"),
    ] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "display-message"));
        }
    }
    let format = match (flags.positional.first(), flags.value('F')) {
        (Some(_), Some(_)) => {
            return Err("only one of -F or argument must be given".to_owned());
        }
        (Some(message), None) => message.clone(),
        (None, Some(format)) => format.to_owned(),
        (None, None) => DISPLAY_MESSAGE_TEMPLATE.to_owned(),
    };
    Ok(InvocationSpec::plain(Request::DisplayMessage {
        target: flags.value('t').map(str::to_owned),
        format,
        print: flags.has('p'),
        literal: flags.has('l'),
    }))
}

fn parse_set_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("set-buffer", arguments, "ab:t:n:w", 1)?;
    for (flag, name) in [('t', "-t"), ('w', "-w")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "set-buffer"));
        }
    }
    let rename = flags.value('n').map(str::to_owned);
    if rename.is_none() && flags.positional.is_empty() {
        return Err("no data specified".to_owned());
    }
    Ok(InvocationSpec::plain(Request::SetBuffer {
        name: flags.value('b').map(str::to_owned),
        append: flags.has('a'),
        data: flags
            .positional
            .first()
            .map(|data| data.clone().into_bytes())
            .unwrap_or_default(),
        rename,
    }))
}

fn parse_list_buffers(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("list-buffers", arguments, "F:f:O:r", 0)?;
    for (flag, name) in [('O', "-O"), ('r', "-r")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "list-buffers"));
        }
    }
    Ok(InvocationSpec::plain(Request::ListBuffers {
        format: flags.value('F').map(str::to_owned),
        filter: flags.value('f').map(str::to_owned),
    }))
}

fn parse_delete_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("delete-buffer", arguments, "b:", 0)?;
    Ok(InvocationSpec::plain(Request::DeleteBuffer {
        name: flags.value('b').map(str::to_owned),
    }))
}

fn parse_paste_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("paste-buffer", arguments, "db:prSs:t:", 0)?;
    if flags.has('S') {
        return Err(unsupported_flag("-S", "paste-buffer"));
    }
    let separator = match flags.value('s') {
        Some(separator) => separator.as_bytes().to_vec(),
        None if flags.has('r') => b"\n".to_vec(),
        None => b"\r".to_vec(),
    };
    Ok(InvocationSpec::plain(Request::PasteBuffer {
        target: flags.value('t').map(str::to_owned),
        name: flags.value('b').map(str::to_owned),
        bracketed: flags.has('p'),
        separator,
        delete: flags.has('d'),
    }))
}

fn parse_load_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("load-buffer", arguments, "b:t:w", 1)?;
    for (flag, name) in [('t', "-t"), ('w', "-w")] {
        if flags.has(flag) {
            return Err(unsupported_flag(name, "load-buffer"));
        }
    }
    let path = flags
        .positional
        .first()
        .ok_or_else(|| "command load-buffer: too few arguments (need at least 1)".to_owned())?;
    let data = if path == "-" {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|error| io_error_text(&error, path))?;
        data
    } else {
        std::fs::read(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::IsADirectory {
                format!("I/O error: {path}")
            } else {
                io_error_text(&error, path)
            }
        })?
    };
    Ok(InvocationSpec::plain(Request::LoadBuffer {
        name: flags.value('b').map(str::to_owned),
        data,
    }))
}

pub(crate) fn io_error_text(error: &std::io::Error, path: &str) -> String {
    let text = error.to_string();
    let reason = text
        .strip_suffix(&format!(" (os error {})", error.raw_os_error().unwrap_or_default()))
        .unwrap_or(&text);
    format!("{reason}: {path}")
}

fn parse_save_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("save-buffer", arguments, "ab:", 1)?;
    let path = flags
        .positional
        .first()
        .ok_or_else(|| "command save-buffer: too few arguments (need at least 1)".to_owned())?;
    let path = if path == "-" || std::path::Path::new(path).is_absolute() {
        path.clone()
    } else {
        current_dir()
            .map(|directory| format!("{directory}/{path}"))
            .unwrap_or_else(|| path.clone())
    };
    Ok(InvocationSpec::plain(Request::SaveBuffer {
        name: flags.value('b').map(str::to_owned),
        path,
        append: flags.has('a'),
    }))
}

fn option_flags(flags: &ScannedFlags<'_>, window_command: bool) -> OptionFlags {
    OptionFlags {
        global: flags.has('g'),
        server: flags.has('s'),
        window: window_command || flags.has('w'),
        pane: flags.has('p'),
    }
}

fn parse_set_option(arguments: &[String]) -> Result<InvocationSpec, String> {
    parse_set_option_command("set-option", arguments, "aFgopqst:uUw", false)
}

fn parse_set_window_option(arguments: &[String]) -> Result<InvocationSpec, String> {
    parse_set_option_command("set-window-option", arguments, "aFgoqt:u", true)
}

fn parse_set_option_command(
    command: &str,
    arguments: &[String],
    accepted: &str,
    window_command: bool,
) -> Result<InvocationSpec, String> {
    let flags = scan_flags(command, arguments, accepted, 2)?;
    for flag in ['a', 'F', 'o', 'U'] {
        if flags.has(flag) {
            return Err(unsupported_flag(&format!("-{flag}"), command));
        }
    }
    let key = flags
        .positional
        .first()
        .ok_or_else(|| format!("command {command}: too few arguments (need at least 1)"))?;
    Ok(InvocationSpec::plain(Request::SetOption {
        target: flags.value('t').map(str::to_owned),
        flags: option_flags(&flags, window_command),
        quiet: flags.has('q'),
        key: key.clone(),
        value: flags.positional.get(1).cloned(),
        unset: flags.has('u'),
    }))
}

fn parse_show_options(
    arguments: &[String],
    window_command: bool,
) -> Result<InvocationSpec, String> {
    let command = if window_command { "show-window-options" } else { "show-options" };
    let accepted = if window_command { "F:gvt:" } else { "AgF:Hpqst:vw" };
    let flags = scan_flags(command, arguments, accepted, 1)?;
    for flag in ['F', 'H'] {
        if flags.has(flag) {
            return Err(unsupported_flag(&format!("-{flag}"), command));
        }
    }
    Ok(InvocationSpec::plain(Request::ShowOptions {
        target: flags.value('t').map(str::to_owned),
        flags: option_flags(&flags, window_command),
        value: flags.has('v'),
        all: flags.has('A'),
        quiet: flags.has('q'),
        key: flags.positional.first().cloned(),
    }))
}

fn parse_set_environment(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("set-environment", arguments, "Fhgrt:u", 2)?;
    for flag in ['F', 'h'] {
        if flags.has(flag) {
            return Err(unsupported_flag(&format!("-{flag}"), "set-environment"));
        }
    }
    let name = flags
        .positional
        .first()
        .ok_or_else(|| "command set-environment: too few arguments (need at least 1)".to_owned())?;
    Ok(InvocationSpec::plain(Request::SetEnvironment {
        target: flags.value('t').map(str::to_owned),
        global: flags.has('g'),
        name: name.clone(),
        value: flags.positional.get(1).cloned(),
        unset: flags.has('u'),
        clear: flags.has('r'),
    }))
}

fn parse_show_environment(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("show-environment", arguments, "hgst:", 1)?;
    if flags.has('h') {
        return Err(unsupported_flag("-h", "show-environment"));
    }
    Ok(InvocationSpec::plain(Request::ShowEnvironment {
        target: flags.value('t').map(str::to_owned),
        global: flags.has('g'),
        shell: flags.has('s'),
        name: flags.positional.first().cloned(),
    }))
}

fn parse_pipe_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("pipe-pane", arguments, "IOot:", 1)?;
    if flags.has('I') {
        return Err(unsupported_flag("-I", "pipe-pane"));
    }
    Ok(InvocationSpec::plain(Request::PipePane {
        target: flags.value('t').map(str::to_owned),
        command: flags.positional.first().cloned(),
        toggle: flags.has('o'),
    }))
}

fn target_and_name(command: &str, arguments: &[String]) -> Result<(Option<String>, String), String> {
    let flags = scan_flags(command, arguments, "t:", 1)?;
    let name = flags
        .positional
        .first()
        .ok_or_else(|| format!("command {command}: too few arguments (need at least 1)"))?;
    Ok((flags.value('t').map(str::to_owned), name.clone()))
}

fn parse_send_prefix(arguments: &[String]) -> Result<InvocationSpec, String> {
    let flags = scan_flags("send-prefix", arguments, "2t:", 0)?;
    if flags.has('2') {
        return Err(unsupported_flag("-2", "send-prefix"));
    }
    Ok(InvocationSpec::plain(Request::SendPrefix {
        target: flags.value('t').map(str::to_owned),
    }))
}

pub(crate) fn unsupported_flag(flag: &str, command: &str) -> String {
    format!("unsupported flag {flag} for {command}")
}

struct ScannedFlags<'a> {
    found: Vec<(char, Option<&'a str>)>,
    positional: &'a [String],
}

impl<'a> ScannedFlags<'a> {
    fn has(&self, flag: char) -> bool {
        self.found.iter().any(|(found, _)| *found == flag)
    }

    fn value(&self, flag: char) -> Option<&'a str> {
        self.found
            .iter()
            .rev()
            .find(|(found, _)| *found == flag)
            .and_then(|(_, value)| *value)
    }
}

fn scan_flags<'a>(
    command: &str,
    arguments: &'a [String],
    accepted: &str,
    most_positional: usize,
) -> Result<ScannedFlags<'a>, String> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" {
            index += 1;
            break;
        }
        let Some(letters) = argument.strip_prefix('-').filter(|letters| !letters.is_empty())
        else {
            break;
        };
        index += 1;
        for (offset, flag) in letters.char_indices() {
            let takes_value = accepted
                .find(flag)
                .map(|at| accepted[at + flag.len_utf8()..].starts_with(':'))
                .ok_or_else(|| unsupported_flag(&format!("-{flag}"), command))?;
            if !takes_value {
                found.push((flag, None));
                continue;
            }
            let optional = accepted
                .find(flag)
                .is_some_and(|at| accepted[at + flag.len_utf8()..].starts_with("::"));
            let attached = &letters[offset + flag.len_utf8()..];
            let value = if attached.is_empty() {
                if optional
                    && arguments.get(index).is_none_or(|next| {
                        let mut bytes = next.bytes();
                        bytes.next() == Some(b'-')
                            && bytes.next().is_some_and(|second| {
                                second == b'-' || second.is_ascii_alphabetic()
                            })
                    })
                {
                    found.push((flag, None));
                    break;
                }
                let value = arguments
                    .get(index)
                    .ok_or_else(|| format!("command {command}: -{flag} expects an argument"))?;
                index += 1;
                value.as_str()
            } else {
                attached
            };
            found.push((flag, Some(value)));
            break;
        }
    }
    let positional = &arguments[index..];
    if positional.len() > most_positional {
        return Err(format!(
            "command {command}: too many arguments (need at most {most_positional})"
        ));
    }
    Ok(ScannedFlags { found, positional })
}

fn argument<'a>(arguments: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    arguments
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires an argument"))
}

fn current_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn key_bytes(key: &str) -> Vec<u8> {
    match key {
        "Enter" | "C-m" | "C-M" => vec![b'\r'],
        "Tab" => vec![b'\t'],
        "BTab" => b"\x1b[Z".to_vec(),
        "Backspace" | "BS" => vec![0x7f],
        "Escape" | "Esc" => vec![0x1b],
        "Space" => vec![b' '],
        "C-Space" => vec![0],
        "Up" => b"\x1b[A".to_vec(),
        "Down" => b"\x1b[B".to_vec(),
        "Right" => b"\x1b[C".to_vec(),
        "Left" => b"\x1b[D".to_vec(),
        "Home" => b"\x1b[H".to_vec(),
        "End" => b"\x1b[F".to_vec(),
        "Delete" => b"\x1b[3~".to_vec(),
        value if value.len() >= 2 && value[..2].eq_ignore_ascii_case("C-") => {
            let byte = value.as_bytes()[2].to_ascii_lowercase();
            vec![if byte == b'@' { 0 } else { byte & 0x1f }]
        }
        value if value.len() >= 2 && value[..2].eq_ignore_ascii_case("M-") => {
            vec![0x1b, value.as_bytes()[2]]
        }
        value => value.as_bytes().to_vec(),
    }
}

fn usage() -> &'static str {
    "usage: tm [-S socket] [command]\n\ncommands: sessions/clients, new/split/new-pane/resize/swap/break/join/respawn, select/kill/rename, send-keys, capture-pane, copy-mode, choose-tree/choose-buffer/choose-client, buffers, options, run-shell, attach/detach/switch, kill-server"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_keeps_the_command_vector_exactly_once() {
        let invocation = parse(&[
            "new-session".to_owned(),
            "-d".to_owned(),
            "-s".to_owned(),
            "build".to_owned(),
            "--".to_owned(),
            "sh".to_owned(),
            "-c".to_owned(),
            "printf ok".to_owned(),
        ])
        .expect("parse");
        let Request::NewSession { command, .. } = invocation.request else {
            panic!("expected new-session request");
        };
        assert_eq!(command, ["sh", "-c", "printf ok"]);
    }

    #[test]
    fn send_keys_maps_tmux_core_key_names() {
        assert_eq!(key_bytes("Enter"), b"\r");
        assert_eq!(key_bytes("C-c"), [3]);
        assert_eq!(key_bytes("Up"), b"\x1b[A");
        assert_eq!(key_bytes("Space"), b" ");
    }

    #[test]
    fn send_keys_repeats_ordinary_keys_for_the_configured_send_binding() {
        let invocation = parse(&[
            "send".to_owned(),
            "-N".to_owned(),
            "2".to_owned(),
            "C-a".to_owned(),
        ])
        .expect("parse repeated send");
        let Request::SendKeys { bytes, .. } = invocation.request else {
            panic!("expected send-keys request");
        };
        assert_eq!(bytes, [1, 1]);
    }

    #[test]
    fn send_keys_copy_actions_preserve_tmux_copy_flags_and_arguments() {
        let invocation = parse(&[
            "send-keys".to_owned(),
            "-t".to_owned(),
            "session:0".to_owned(),
            "-X".to_owned(),
            "copy-pipe-and-cancel".to_owned(),
            "-P".to_owned(),
            "-C".to_owned(),
            "cat".to_owned(),
            "named-".to_owned(),
        ])
        .expect("parse copy action flags");
        let Request::CopyModeCommand { target, action, .. } = invocation.request else {
            panic!("expected copy-mode request");
        };
        assert_eq!(target.as_deref(), Some("session:0"));
        assert_eq!(action, "copy-pipe-and-cancel\x1dPC\0cat\x1enamed-");
    }

    #[test]
    fn paste_buffer_parses_bracketed_paste_mode() {
        let invocation = parse(&[
            "paste-buffer".to_owned(),
            "-p".to_owned(),
            "-b".to_owned(),
            "paste".to_owned(),
        ])
        .expect("parse bracketed paste");
        assert!(matches!(
            invocation.request,
            Request::PasteBuffer {
                name: Some(name),
                bracketed: true,
                ..
            } if name == "paste"
        ));
    }

    #[test]
    fn select_window_navigation_forms_map_to_window_requests() {
        let next = parse(&["select-window".to_owned(), "-n".to_owned()]).expect("next");
        assert!(matches!(
            next.request,
            Request::SelectWindow { target, step: WindowStep::Next } if target.is_empty()
        ));
        let previous = parse(&[
            "selectw".to_owned(),
            "-p".to_owned(),
            "-t".to_owned(),
            "S:".to_owned(),
        ])
        .expect("previous");
        assert!(matches!(
            previous.request,
            Request::SelectWindow { target, step: WindowStep::Previous } if target == "S:"
        ));
        let last = parse(&["last-window".to_owned()]).expect("last");
        assert!(matches!(
            last.request,
            Request::SelectWindow { target, step: WindowStep::Last } if target.is_empty()
        ));
        assert!(parse(&["kill-window".to_owned(), "-f".to_owned()]).is_err());
    }

    #[test]
    fn choose_commands_take_only_a_target() {
        for command in ["choose-tree", "choose-buffer", "choose-client"] {
            let invocation = parse(&[command.to_owned(), "-t".to_owned(), "work:1".to_owned()])
                .expect(command);
            assert!(matches!(
                invocation.request,
                Request::ChooseTree { target: Some(ref target), .. }
                    | Request::ChooseBuffer { target: Some(ref target), .. }
                    | Request::ChooseClient { target: Some(ref target), .. }
                    if target == "work:1"
            ));
            assert_eq!(
                parse(&[command.to_owned(), "-f".to_owned(), "1".to_owned()]).err(),
                Some(unsupported_flag("-f", command))
            );
        }
    }

    #[test]
    fn display_panes_takes_a_target_and_one_template_string() {
        let invocation = parse(&[
            "display-panes".to_owned(),
            "-t".to_owned(),
            "%3".to_owned(),
            "set-option -g @picked %%".to_owned(),
        ])
        .expect("display-panes");
        assert!(matches!(
            invocation.request,
            Request::DisplayPanes {
                target: Some(target),
                command,
                ..
            } if target == "%3" && command == ["set-option", "-g", "@picked", "%%"]
        ));
    }

    #[test]
    fn copy_mode_parses_page_direction_and_reset_options() {
        let page_down = parse(&["copy-mode".to_owned(), "-d".to_owned()]).expect("page down");
        assert!(matches!(
            page_down.request,
            Request::CopyMode {
                page: false,
                page_down: true,
                reset: false,
                ..
            }
        ));
        let reset = parse(&["copy-mode".to_owned(), "-q".to_owned()]).expect("reset");
        assert!(matches!(
            reset.request,
            Request::CopyMode {
                page_down: false,
                reset: true,
                ..
            }
        ));
        let source = parse(&[
            "copy-mode".to_owned(),
            "-s".to_owned(),
            "work:1.2".to_owned(),
        ])
        .expect("copy source");
        assert!(matches!(
            source.request,
            Request::CopyMode {
                source: Some(source),
                ..
            } if source == "work:1.2"
        ));
        for flag in ["-k", "-M", "-S"] {
            assert_eq!(
                parse(&["copy-mode".to_owned(), flag.to_owned()]).err(),
                Some(unsupported_flag(flag, "copy-mode"))
            );
        }
    }
}
