use std::io::Read;
use std::path::PathBuf;

use crate::model::Size;
use crate::protocol::{OptionScope, PaneDirection, Request};
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
        Some("refresh-client" | "refreshc") => parse_refresh_client(&arguments[index + 1..])?,
        Some("run-shell" | "run") => parse_run_shell(&arguments[index + 1..])?,
        Some("list-sessions" | "ls") => parse_list_sessions(&arguments[index + 1..])?,
        Some("list-clients" | "lsc") => parse_list_clients(&arguments[index + 1..])?,
        Some("has-session" | "has") => {
            let target = required_target(&arguments[index + 1..])?;
            InvocationSpec::plain(Request::HasSession { target })
        }
        Some("kill-session" | "kill") => parse_kill_session(&arguments[index + 1..])?,
        Some("new-window" | "neww") => parse_new_window(&arguments[index + 1..])?,
        Some("split-window" | "splitw" | "split" | "new-pane" | "newp") => {
            parse_split_window(&arguments[index + 1..])?
        }
        Some("resize-pane" | "resizep") => parse_resize_pane(&arguments[index + 1..])?,
        Some("swap-pane" | "swapp") => parse_swap_pane(&arguments[index + 1..])?,
        Some("break-pane" | "breakp") => parse_break_pane(&arguments[index + 1..])?,
        Some("join-pane" | "joinp" | "move-pane" | "movep") => {
            parse_join_pane(&arguments[index + 1..])?
        }
        Some("respawn-pane" | "respawnp") => parse_respawn(&arguments[index + 1..], false)?,
        Some("respawn-window" | "respawnw") => parse_respawn(&arguments[index + 1..], true)?,
        Some("clear-history" | "clearhist") => InvocationSpec::plain(Request::ClearHistory {
            target: optional_target(&arguments[index + 1..])?,
        }),
        Some("rotate-window" | "rotatew") => parse_rotate_window(&arguments[index + 1..])?,
        Some("swap-window" | "swapw") => parse_swap_window(&arguments[index + 1..])?,
        Some("move-window" | "movew") => parse_move_window(&arguments[index + 1..])?,
        Some("link-window" | "linkw") => parse_link_window(&arguments[index + 1..])?,
        Some("unlink-window" | "unlinkw") => parse_unlink_window(&arguments[index + 1..])?,
        Some("list-windows" | "lsw") => parse_list_windows(&arguments[index + 1..])?,
        Some("list-panes" | "lsp") => parse_list_panes(&arguments[index + 1..])?,
        Some("select-window" | "selectw") => parse_select_window(&arguments[index + 1..])?,
        Some("last-window" | "last") => InvocationSpec::plain(Request::SelectWindow {
            target: "!".to_owned(),
        }),
        Some("next-window" | "next") => InvocationSpec::plain(Request::NextWindow {
            target: optional_target(&arguments[index + 1..])?,
        }),
        Some("previous-window" | "prev") => InvocationSpec::plain(Request::PreviousWindow {
            target: optional_target(&arguments[index + 1..])?,
        }),
        Some("rename-session" | "rename") => {
            let (target, name) = target_and_name(&arguments[index + 1..])?;
            InvocationSpec::plain(Request::RenameSession { target, name })
        }
        Some("rename-window" | "renamew") => {
            let (target, name) = target_and_name(&arguments[index + 1..])?;
            InvocationSpec::plain(Request::RenameWindow { target, name })
        }
        Some("kill-window" | "killw") => parse_kill_window(&arguments[index + 1..])?,
        Some("select-pane" | "selectp") => parse_select_pane(&arguments[index + 1..])?,
        Some("kill-pane" | "killp") => parse_kill_pane(&arguments[index + 1..])?,
        Some("send-keys" | "send") => parse_send_keys(&arguments[index + 1..])?,
        Some("send-prefix") => InvocationSpec::plain(Request::SendKeys {
            target: optional_target(&arguments[index + 1..])?,
            bytes: vec![2],
            reset: false,
        }),
        Some("capture-pane" | "capturep") => parse_capture_pane(&arguments[index + 1..])?,
        Some("copy-mode") => parse_copy_mode(&arguments[index + 1..], false)?,
        Some("copy-mode-and-page") => parse_copy_mode(&arguments[index + 1..], true)?,
        Some("display-panes") => parse_display_panes(&arguments[index + 1..])?,
        Some("choose-tree" | "choose") => parse_choose_tree(&arguments[index + 1..])?,
        Some("choose-buffer" | "chooseb") => parse_choose_buffer(&arguments[index + 1..])?,
        Some("choose-client" | "choosec") => parse_choose_client(&arguments[index + 1..])?,
        Some("display-message" | "display") => parse_display_message(&arguments[index + 1..])?,
        Some("set-buffer" | "setb") => parse_set_buffer(&arguments[index + 1..])?,
        Some("show-buffer" | "showb") => InvocationSpec::plain(Request::ShowBuffer {
            name: parse_buffer_name(&arguments[index + 1..])?,
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
        Some("kill-server") => InvocationSpec::plain(Request::KillServer),
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
            empty: false,
            command: Vec::new(),
            cwd: current_dir(),
            size: Size::new(80, 24),
        },
        attach: true,
    }
}

fn parse_new_session(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut name = None;
    let mut window_name = None;
    let mut cwd = current_dir();
    let mut detached = false;
    let mut attach_existing = false;
    let mut group_target = None;
    let mut format = None;
    let mut empty = false;
    let mut command = Vec::new();
    let mut size = Size::new(80, 24);
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-d" => detached = true,
            "-A" => attach_existing = true,
            "-t" => {
                index += 1;
                group_target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-P" => format = Some("#{session_name}".to_owned()),
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-s" => {
                index += 1;
                name = Some(argument(arguments, index, "-s")?.to_owned());
            }
            value if value.starts_with("-ds") && value.len() > 3 => {
                detached = true;
                name = Some(value[3..].to_owned());
            }
            value if value.starts_with("-s") && value.len() > 2 => {
                name = Some(value[2..].to_owned());
            }
            "-n" => {
                index += 1;
                window_name = Some(argument(arguments, index, "-n")?.to_owned());
            }
            "-E" => empty = true,
            value if value.starts_with("-n") && value.len() > 2 => {
                window_name = Some(value[2..].to_owned());
            }
            "-c" => {
                index += 1;
                cwd = Some(argument(arguments, index, "-c")?.to_owned());
            }
            "-x" => {
                index += 1;
                size.cols = parse_dimension(argument(arguments, index, "-x")?, "-x")?;
            }
            "-y" => {
                index += 1;
                size.rows = parse_dimension(argument(arguments, index, "-y")?, "-y")?;
            }
            value if value.starts_with("-x") && value.len() > 2 => {
                size.cols = parse_dimension(&value[2..], "-x")?;
            }
            value if value.starts_with("-y") && value.len() > 2 => {
                size.rows = parse_dimension(&value[2..], "-y")?;
            }
            "--" => command.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') => {
                return Err(format!("unknown new-session option: {value}"));
            }
            _ => {
                command.extend_from_slice(&arguments[index..]);
                break;
            }
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
    }
    if command.len() == 1 && command[0].is_empty() {
        command.clear();
        empty = true;
    }
    if empty && !command.is_empty() {
        return Err("command cannot be given for empty pane".to_owned());
    }
    Ok(InvocationSpec {
        request: Request::NewSession {
            name,
            detached,
            attach_existing,
            group_target,
            format,
            window_name,
            empty,
            command,
            cwd,
            size,
        },
        attach: !detached,
    })
}

fn parse_attach(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut size = Size::new(80, 24);
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-x" => {
                index += 1;
                size.cols = parse_dimension(argument(arguments, index, "-x")?, "-x")?;
            }
            "-y" => {
                index += 1;
                size.rows = parse_dimension(argument(arguments, index, "-y")?, "-y")?;
            }
            value if value.starts_with("-x") && value.len() > 2 => {
                size.cols = parse_dimension(&value[2..], "-x")?;
            }
            value if value.starts_with("-y") && value.len() > 2 => {
                size.rows = parse_dimension(&value[2..], "-y")?;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown attach option: {value}"));
            }
            value => target = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec {
        request: Request::Attach {
            target: target.clone(),
            size,
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
    let mut target = None;
    let mut all = false;
    let mut force = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-a" => all = true,
            "-C" => {}
            "-f" => force = true,
            value if !value.starts_with('-') => target = Some(value.to_owned()),
            value => return Err(format!("unknown kill-session option: {value}")),
        }
        index += 1;
    }
    if force && !all {
        return Err("-f only valid with -a".to_owned());
    }
    Ok(InvocationSpec::plain(Request::KillSession { target, all }))
}

fn parse_kill_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut all = false;
    let mut force = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-a" => all = true,
            "-f" => force = true,
            value if !value.starts_with('-') => target = Some(value.to_owned()),
            value => return Err(format!("unknown kill-window option: {value}")),
        }
        index += 1;
    }
    if force && !all {
        return Err("-f only valid with -a".to_owned());
    }
    Ok(InvocationSpec::plain(Request::KillWindow { target, all }))
}

fn parse_new_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut name = None;
    let mut cwd = None;
    let mut detached = false;
    let mut index = None;
    let mut force = false;
    let mut format = None;
    let mut after = false;
    let mut before = false;
    let mut select_existing = false;
    let mut empty = false;
    let (command, _) = parse_common_tail(arguments, |flag, value| match flag {
        "-t" => {
            target = Some(value.to_owned());
            Ok(())
        }
        "-n" => {
            name = Some(value.to_owned());
            Ok(())
        }
        "-c" => {
            cwd = Some(value.to_owned());
            Ok(())
        }
        "-d" => {
            detached = true;
            Ok(())
        }
        "-k" => {
            force = true;
            Ok(())
        }
        "-P" => {
            format = Some("#{window_index}:#{window_name}".to_owned());
            Ok(())
        }
        "-F" => {
            format = Some(value.to_owned());
            Ok(())
        }
        "-I" => {
            index = Some(
                value
                    .parse::<u32>()
                    .map_err(|_| "new-window index requires an integer".to_owned())?,
            );
            Ok(())
        }
        "-a" => {
            after = true;
            Ok(())
        }
        "-b" => {
            before = true;
            Ok(())
        }
        "-S" => {
            select_existing = true;
            Ok(())
        }
        "-E" => {
            empty = true;
            Ok(())
        }
        other => return Err(format!("unknown new-window option: {other}")),
    })?;
    let (command, empty) = if command.len() == 1 && command[0].is_empty() {
        (Vec::new(), true)
    } else {
        if empty && !command.is_empty() {
            return Err("command cannot be given for empty pane".to_owned());
        }
        (command, empty)
    };
    Ok(InvocationSpec {
        request: Request::NewWindow {
            target: target.clone(),
            name,
            detached,
            empty,
            index,
            force,
            format,
            after,
            before,
            select_existing,
            command,
            cwd,
        },
        attach: !detached,
    })
}

fn parse_split_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut cwd = None;
    let mut horizontal = false;
    let mut before = false;
    let mut full = false;
    let mut detached = false;
    let mut zoom = false;
    let mut size = None;
    let mut empty = false;
    let (command, _) = parse_common_tail(arguments, |flag, value| match flag {
        "-t" => {
            target = Some(value.to_owned());
            Ok(())
        }
        "-c" => {
            cwd = Some(value.to_owned());
            Ok(())
        }
        "-h" => {
            horizontal = true;
            Ok(())
        }
        "-v" => Ok(()),
        "-d" => {
            detached = true;
            Ok(())
        }
        "-b" => {
            before = true;
            Ok(())
        }
        "-f" => {
            full = true;
            Ok(())
        }
        "-Z" => {
            zoom = true;
            Ok(())
        }
        "-l" => {
            size = Some(value.to_owned());
            Ok(())
        }
        "-E" => {
            empty = true;
            Ok(())
        }
        other => return Err(format!("unknown split-window option: {other}")),
    })?;
    let (command, empty) = if command.len() == 1 && command[0].is_empty() {
        (Vec::new(), true)
    } else {
        if empty && !command.is_empty() {
            return Err("command cannot be given for empty pane".to_owned());
        }
        (command, empty)
    };
    Ok(InvocationSpec::plain(Request::SplitWindow {
        target,
        horizontal,
        before,
        full,
        detached,
        empty,
        zoom,
        size,
        command,
        cwd,
    }))
}

fn parse_list_sessions(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-f" | "-a" => {
                if arguments[index] == "-f" {
                    index += 1;
                    let _ = argument(arguments, index, "-f")?;
                }
            }
            value => return Err(format!("unknown list-sessions option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ListSessions { format }))
}

fn parse_list_clients(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-f" => {
                index += 1;
                let _ = argument(arguments, index, "-f")?;
            }
            value => return Err(format!("unknown list-clients option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ListClients { format }))
}

fn parse_detach_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut all = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-a" => all = true,
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown detach-client option: {value}"));
            }
            value => target = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::DetachClient { target, all }))
}

fn parse_switch_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut client = None;
    let mut session = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-c" => {
                index += 1;
                client = Some(argument(arguments, index, "-c")?.to_owned());
            }
            "-t" => {
                index += 1;
                session = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "--" => {
                if session.is_none() {
                    session = arguments.get(index + 1).cloned();
                }
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown switch-client option: {value}"));
            }
            value => session = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SwitchClient {
        client,
        session: session.ok_or_else(|| "switch-client requires a target session".to_owned())?,
    }))
}

fn parse_refresh_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-S" | "-l" | "-r" => {}
            value if value.starts_with('-') => {
                return Err(format!("unknown refresh-client option: {value}"));
            }
            value => target = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::RefreshClient { target }))
}

fn parse_run_shell(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut background = false;
    let mut target = None;
    let mut command = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => background = true,
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-d" => {
                index += 1;
                let _ = argument(arguments, index, "-d")?;
            }
            "--" => {
                command.extend_from_slice(&arguments[index + 1..]);
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown run-shell option: {value}"));
            }
            _ => {
                command.extend_from_slice(&arguments[index..]);
                break;
            }
        }
        index += 1;
    }
    if command.is_empty() {
        return Err("run-shell requires a command".to_owned());
    }
    Ok(InvocationSpec::plain(Request::RunShell {
        command: command.join(" "),
        background,
        target,
    }))
}

fn parse_select_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut navigation = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-n" => navigation = Some(true),
            "-p" => navigation = Some(false),
            "-l" => {
                if navigation.is_some() {
                    return Err("select-window accepts only one navigation option".to_owned());
                }
                target = Some("!".to_owned());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown select-window option: {value}"));
            }
            value => target = Some(value.to_owned()),
        }
        index += 1;
    }
    if let Some(next) = navigation {
        return Ok(InvocationSpec::plain(if next {
            Request::NextWindow { target }
        } else {
            Request::PreviousWindow { target }
        }));
    }
    Ok(InvocationSpec::plain(Request::SelectWindow {
        target: target.ok_or_else(|| "select-window requires a target".to_owned())?,
    }))
}

fn parse_rotate_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut up = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-U" => up = true,
            "-D" => up = false,
            value => return Err(format!("unknown rotate-window option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::RotateWindow { target, up }))
}

fn parse_swap_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut detached = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-d" => detached = true,
            "-r" => {}
            value => return Err(format!("unknown swap-window option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SwapWindow {
        source,
        target,
        detached,
    }))
}

fn parse_move_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut after = false;
    let mut detached = false;
    let mut force = false;
    let mut renumber = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-a" => after = true,
            "-d" => detached = true,
            "-k" => force = true,
            "-r" => renumber = true,
            value => return Err(format!("unknown move-window option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::MoveWindow {
        source,
        target,
        after,
        detached,
        force,
        renumber,
    }))
}

fn parse_link_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut detached = false;
    let mut force = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-d" => detached = true,
            "-k" => force = true,
            value => return Err(format!("unknown link-window option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::LinkWindow {
        source,
        target,
        detached,
        force,
    }))
}

fn parse_unlink_window(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut force = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-k" => force = true,
            value => return Err(format!("unknown unlink-window option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::UnlinkWindow {
        target,
        force,
    }))
}

fn parse_list_windows(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-f" => {
                index += 1;
                let _ = argument(arguments, index, "-f")?;
            }
            value => return Err(format!("unknown list-windows option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ListWindows {
        target,
        format,
    }))
}

fn parse_list_panes(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-a" | "-s" => {}
            "-f" => {
                index += 1;
                let _ = argument(arguments, index, "-f")?;
            }
            value => return Err(format!("unknown list-panes option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ListPanes { target, format }))
}

fn parse_select_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut direction = PaneDirection::Last;
    let mut mark = None;
    let mut title = None;
    let mut enabled = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-L" => direction = PaneDirection::Left,
            "-R" => direction = PaneDirection::Right,
            "-U" => direction = PaneDirection::Up,
            "-D" => direction = PaneDirection::Down,
            "-l" => direction = PaneDirection::Last,
            "-n" => direction = PaneDirection::Next,
            "-p" => direction = PaneDirection::Previous,
            "-m" => mark = Some(true),
            "-M" => mark = Some(false),
            "-d" => enabled = Some(false),
            "-e" => enabled = Some(true),
            "-T" => {
                index += 1;
                title = Some(argument(arguments, index, "-T")?.to_owned());
            }
            value => return Err(format!("unknown select-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SelectPane {
        target,
        direction,
        mark,
        title,
        enabled,
    }))
}

fn parse_kill_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut all = false;
    let mut filter = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-a" => all = true,
            "-f" => {
                index += 1;
                filter = Some(argument(arguments, index, "-f")?.to_owned());
            }
            "-K" | "-F" => {
                if arguments[index] == "-F" {
                    index += 1;
                    let _ = argument(arguments, index, "-F")?;
                }
            }
            value => return Err(format!("unknown kill-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::KillPane {
        target,
        all,
        filter,
    }))
}

fn parse_resize_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut direction = PaneDirection::Right;
    let mut amount = 1;
    let mut absolute = None;
    let mut absolute_percent = false;
    let mut zoom = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-L" => direction = PaneDirection::Left,
            "-R" => direction = PaneDirection::Right,
            "-U" => direction = PaneDirection::Up,
            "-D" => direction = PaneDirection::Down,
            "-Z" => zoom = true,
            "-x" => {
                index += 1;
                direction = PaneDirection::Right;
                let (value, percent) = parse_resize_value(argument(arguments, index, "-x")?)?;
                absolute = Some(value);
                absolute_percent = percent;
            }
            "-y" => {
                index += 1;
                direction = PaneDirection::Down;
                let (value, percent) = parse_resize_value(argument(arguments, index, "-y")?)?;
                absolute = Some(value);
                absolute_percent = percent;
            }
            value if value.starts_with("-x") && value.len() > 2 => {
                direction = PaneDirection::Right;
                let (value, percent) = parse_resize_value(&value[2..])?;
                absolute = Some(value);
                absolute_percent = percent;
            }
            value if value.starts_with("-y") && value.len() > 2 => {
                direction = PaneDirection::Down;
                let (value, percent) = parse_resize_value(&value[2..])?;
                absolute = Some(value);
                absolute_percent = percent;
            }
            value if !value.starts_with('-') => {
                amount = value
                    .parse::<i32>()
                    .map_err(|_| "resize-pane count requires an integer".to_owned())?;
            }
            value => return Err(format!("unknown resize-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ResizePane {
        target,
        direction,
        amount,
        absolute,
        absolute_percent,
        zoom,
    }))
}

fn parse_resize_value(value: &str) -> Result<(u16, bool), String> {
    let percent = value.ends_with('%');
    Ok((
        value
            .strip_suffix('%')
            .unwrap_or(value)
            .parse::<u16>()
            .map_err(|_| "resize-pane size requires an integer".to_owned())?,
        percent,
    ))
}

fn parse_swap_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut direction = None;
    let mut detached = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-U" => direction = Some(PaneDirection::Previous),
            "-D" => direction = Some(PaneDirection::Next),
            "-d" => detached = true,
            "-Z" => {}
            value => return Err(format!("unknown swap-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SwapPane {
        source,
        target,
        direction,
        detached,
    }))
}

fn parse_break_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut name = None;
    let mut detached = false;
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-n" => {
                index += 1;
                name = Some(argument(arguments, index, "-n")?.to_owned());
            }
            "-d" => detached = true,
            "-P" => {
                format = Some("#{window_index}:#{pane_id}".to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            value => return Err(format!("unknown break-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::BreakPane {
        source,
        target,
        name,
        detached,
        format,
    }))
}

fn parse_join_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut horizontal = false;
    let mut before = false;
    let mut detached = false;
    let mut size = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-h" => horizontal = true,
            "-v" => {}
            "-b" => before = true,
            "-d" => detached = true,
            "-l" => {
                index += 1;
                size = Some(argument(arguments, index, "-l")?.to_owned());
            }
            value => return Err(format!("unknown join-pane option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::JoinPane {
        source,
        target,
        horizontal,
        before,
        detached,
        size,
    }))
}

fn parse_respawn(arguments: &[String], window: bool) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut cwd = None;
    let mut kill = false;
    let mut empty = false;
    let (command, _) = parse_common_tail(arguments, |flag, value| match flag {
        "-t" => {
            target = Some(value.to_owned());
            Ok(())
        }
        "-c" => {
            cwd = Some(value.to_owned());
            Ok(())
        }
        "-k" => {
            kill = true;
            Ok(())
        }
        "-d" => Ok(()),
        "-E" => {
            empty = true;
            Ok(())
        }
        other => Err(format!("unknown respawn option: {other}")),
    })?;
    let (command, empty) = if command.len() == 1 && command[0].is_empty() {
        (Vec::new(), true)
    } else {
        (command, empty)
    };
    Ok(InvocationSpec::plain(Request::RespawnPane {
        target,
        command,
        cwd,
        kill,
        empty,
        window,
    }))
}

fn parse_send_keys(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut literal = false;
    let mut reset = false;
    let mut copy_action = None;
    let mut copy_no_paste = false;
    let mut copy_no_clipboard = false;
    let mut repeat = 1;
    let mut keys = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-l" => literal = true,
            "-R" => reset = true,
            "-X" => {
                index += 1;
                copy_action = Some(argument(arguments, index, "-X")?.to_owned());
            }
            "-P" if copy_action.is_some() => copy_no_paste = true,
            "-C" if copy_action.is_some() => copy_no_clipboard = true,
            "-N" => {
                index += 1;
                repeat = argument(arguments, index, "-N")?
                    .parse::<u32>()
                    .map_err(|_| "-N requires a positive integer".to_owned())?;
                if repeat == 0 {
                    return Err("-N requires a positive integer".to_owned());
                }
            }
            value if value.starts_with("-N") && value.len() > 2 => {
                repeat = value[2..]
                    .parse::<u32>()
                    .map_err(|_| "-N requires a positive integer".to_owned())?;
                if repeat == 0 {
                    return Err("-N requires a positive integer".to_owned());
                }
            }
            "--" => keys.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') && !literal => {
                return Err(format!("unknown send-keys option: {value}"));
            }
            value => keys.push(value.to_owned()),
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
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
        keys.join(" ").into_bytes()
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

fn parse_copy_mode(arguments: &[String], mut page: bool) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut source = None;
    let mut exit_on_scroll = false;
    let mut hide_position = false;
    let mut kill_on_exit = false;
    let mut page_down = false;
    let mut reset = false;
    let mut mouse_start = false;
    let mut scroll_to_mouse = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-e" => exit_on_scroll = true,
            "-H" => hide_position = true,
            "-k" => kill_on_exit = true,
            "-d" => page_down = true,
            "-u" => page = true,
            "-q" => reset = true,
            "-M" => mouse_start = true,
            "-S" => scroll_to_mouse = true,
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            value => return Err(format!("unknown copy-mode option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::CopyMode {
        target,
        source,
        exit_on_scroll,
        hide_position,
        kill_on_exit,
        page,
        page_down,
        reset,
        mouse_start,
        scroll_to_mouse,
    }))
}

fn parse_display_panes(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut source = None;
    let mut no_zoom = false;
    let mut no_mode = false;
    let mut kill_on_exit = false;
    let mut command = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-s" => {
                index += 1;
                source = Some(argument(arguments, index, "-s")?.to_owned());
            }
            "-Z" => no_zoom = true,
            "-k" => kill_on_exit = true,
            "-N" => no_mode = true,
            "-Nd" => {
                no_mode = true;
                if arguments
                    .get(index + 1)
                    .is_some_and(|value| !value.starts_with('-'))
                {
                    index += 1;
                }
            }
            "-Zd" => {
                no_zoom = true;
                if arguments
                    .get(index + 1)
                    .is_some_and(|value| !value.starts_with('-'))
                {
                    index += 1;
                }
            }
            "-d" => {
                if arguments
                    .get(index + 1)
                    .is_some_and(|value| !value.starts_with('-'))
                {
                    index += 1;
                }
            }
            value if value.starts_with("-d") && value.len() > 2 => {}
            value if value.starts_with('-') => {
                return Err(format!("unknown display-panes option: {value}"));
            }
            _ => {
                command.extend_from_slice(&arguments[index..]);
                break;
            }
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::DisplayPanes {
        target,
        source,
        no_zoom,
        no_mode,
        command,
        kill_on_exit,
    }))
}

fn parse_choose_tree(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut filter = None;
    let mut format = None;
    let mut sort = "index".to_owned();
    let mut reverse = false;
    let mut hide_source = false;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-f" => {
                index += 1;
                filter = Some(argument(arguments, index, "-f")?.to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-O" => {
                index += 1;
                sort = argument(arguments, index, "-O")?.to_owned();
            }
            "-r" => reverse = true,
            "-h" => hide_source = true,
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-tree option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ChooseTree {
        target,
        filter,
        format,
        sort,
        reverse,
        hide_source,
        kill_on_exit,
    }))
}

fn parse_choose_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let (target, filter, format, sort, reverse, kill_on_exit) =
        parse_choose_list_options(arguments, true)?;
    Ok(InvocationSpec::plain(Request::ChooseBuffer {
        target,
        filter,
        format,
        sort,
        reverse,
        kill_on_exit,
    }))
}

fn parse_choose_client(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut filter = None;
    let mut format = None;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-f" => {
                index += 1;
                filter = Some(argument(arguments, index, "-f")?.to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-client option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ChooseClient {
        target,
        filter,
        format,
        kill_on_exit,
    }))
}

fn parse_choose_list_options(
    arguments: &[String],
    allow_sort: bool,
) -> Result<
    (
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        bool,
        bool,
    ),
    String,
> {
    let mut target = None;
    let mut filter = None;
    let mut format = None;
    let mut sort = "index".to_owned();
    let mut reverse = false;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-f" => {
                index += 1;
                filter = Some(argument(arguments, index, "-f")?.to_owned());
            }
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-O" if allow_sort => {
                index += 1;
                sort = argument(arguments, index, "-O")?.to_owned();
            }
            "-r" if allow_sort => reverse = true,
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-buffer option: {value}")),
        }
        index += 1;
    }
    Ok((target, filter, format, sort, reverse, kill_on_exit))
}

fn parse_capture_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut start = None;
    let mut end = None;
    let mut escape = false;
    let mut join = false;
    let mut preserve_trailing = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-p" => {}
            "-e" => escape = true,
            "-J" => join = true,
            "-N" => preserve_trailing = true,
            "-H" => {}
            "-S" => {
                index += 1;
                start = Some(parse_capture_offset(argument(arguments, index, "-S")?)?);
            }
            "-E" => {
                index += 1;
                end = Some(parse_capture_offset(argument(arguments, index, "-E")?)?);
            }
            value if value.starts_with("-S") && value.len() > 2 => {
                start = Some(parse_capture_offset(&value[2..])?);
            }
            value if value.starts_with("-E") && value.len() > 2 => {
                end = Some(parse_capture_offset(&value[2..])?);
            }
            value if value.starts_with("-p") && value.len() > 2 => {
                for flag in value[2..].chars() {
                    match flag {
                        'e' => escape = true,
                        'J' => join = true,
                        'N' => preserve_trailing = true,
                        'H' => {}
                        'p' => {}
                        other => return Err(format!("unknown capture-pane option: -{other}")),
                    }
                }
            }
            value => return Err(format!("unknown capture-pane option: {value}")),
        }
        index += 1;
    }
    preserve_trailing |= join;
    Ok(InvocationSpec::plain(Request::CapturePane {
        target,
        start,
        end,
        escape,
        join,
        preserve_trailing,
    }))
}

fn parse_capture_offset(value: &str) -> Result<i32, String> {
    if value == "-" {
        return Ok(i32::MIN);
    }
    value
        .parse::<i32>()
        .map_err(|_| "capture-pane offset requires an integer".to_owned())
}

fn parse_display_message(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-p" | "-v" => {}
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "--" => {
                format = arguments.get(index + 1).cloned();
                break;
            }
            value => format = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::DisplayMessage {
        target,
        format: format.ok_or_else(|| "display-message requires a format".to_owned())?,
    }))
}

fn parse_set_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut name = None;
    let mut rename = None;
    let mut append = false;
    let mut data = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            "-n" => {
                index += 1;
                rename = Some(argument(arguments, index, "-n")?.to_owned());
            }
            "-a" => append = true,
            "--" => data.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') => {
                return Err(format!("unknown set-buffer option: {value}"));
            }
            value => data.push(value.to_owned()),
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SetBuffer {
        name,
        append,
        data: data.join(" ").into_bytes(),
        rename,
    }))
}

fn parse_list_buffers(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut format = None;
    let mut filter = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-f" => {
                index += 1;
                filter = Some(argument(arguments, index, "-f")?.to_owned());
            }
            "-q" => {}
            value => return Err(format!("unknown list-buffers option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ListBuffers {
        format,
        filter,
    }))
}

fn parse_delete_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut name = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            value => return Err(format!("unknown delete-buffer option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::DeleteBuffer { name }))
}

fn parse_paste_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut name = None;
    let mut raw = false;
    let mut bracketed = false;
    let mut separator = None;
    let mut delete = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            "-r" => raw = true,
            "-p" => bracketed = true,
            "-d" => delete = true,
            "-s" => {
                index += 1;
                separator = Some(argument(arguments, index, "-s")?.as_bytes().to_vec());
            }
            value => return Err(format!("unknown paste-buffer option: {value}")),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::PasteBuffer {
        target,
        name,
        raw,
        bracketed,
        separator,
        delete,
    }))
}

fn parse_load_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut name = None;
    let mut path = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            value if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown load-buffer option: {value}"));
            }
            value => path = Some(value.to_owned()),
        }
        index += 1;
    }
    let path = path.ok_or_else(|| "load-buffer requires a file".to_owned())?;
    let data = if path == "-" {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|error| error.to_string())?;
        data
    } else {
        std::fs::read(&path).map_err(|error| format!("{}: {error}", path))?
    };
    Ok(InvocationSpec::plain(Request::LoadBuffer { name, data }))
}

fn parse_save_buffer(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut name = None;
    let mut path = None;
    let mut append = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            "-a" => append = true,
            value if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown save-buffer option: {value}"));
            }
            value => path = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::SaveBuffer {
        name,
        path,
        append,
    }))
}

fn parse_buffer_name(arguments: &[String]) -> Result<Option<String>, String> {
    let mut name = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-b" => {
                index += 1;
                name = Some(argument(arguments, index, "-b")?.to_owned());
            }
            "--" => break,
            value => return Err(format!("unknown show-buffer option: {value}")),
        }
        index += 1;
    }
    Ok(name)
}

fn parse_set_window_option(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut global = false;
    let mut unset = false;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-g" => global = true,
            "-w" => {}
            "-u" => unset = true,
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "--" => positional.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') => {
                return Err(format!("unknown set-window-option option: {value}"));
            }
            value => positional.push(value.to_owned()),
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
    }
    let key = positional
        .first()
        .cloned()
        .ok_or_else(|| "set-window-option requires an option".to_owned())?;
    let value = positional.get(1).cloned().unwrap_or_default();
    if !unset && positional.get(1).is_none() {
        return Err("set-window-option requires a value".to_owned());
    }
    if global {
        return Ok(InvocationSpec::plain(Request::SetOption {
            target: None,
            scope: Some(OptionScope::Global),
            key,
            value,
            unset,
        }));
    }
    if unset {
        return Ok(InvocationSpec::plain(Request::SetOption {
            target,
            scope: Some(OptionScope::Window),
            key,
            value,
            unset,
        }));
    }
    Ok(InvocationSpec::plain(Request::SetWindowOption {
        target,
        key,
        value,
    }))
}

fn parse_set_option(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut scope = None;
    let mut unset = false;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-g" => scope = Some(OptionScope::Global),
            "-s" => scope = Some(OptionScope::Session),
            "-w" => scope = Some(OptionScope::Window),
            "-p" => scope = Some(OptionScope::Pane),
            "-u" => unset = true,
            "-q" => {}
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "--" => positional.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') && value.len() > 2 => {
                for flag in value[1..].chars() {
                    match flag {
                        'g' => scope = Some(OptionScope::Global),
                        's' => scope = Some(OptionScope::Session),
                        'w' => scope = Some(OptionScope::Window),
                        'p' => scope = Some(OptionScope::Pane),
                        'u' => unset = true,
                        'q' => {}
                        other => return Err(format!("unknown set-option option: -{other}")),
                    }
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown set-option option: {value}"));
            }
            value => positional.push(value.to_owned()),
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
    }
    let key = positional
        .first()
        .cloned()
        .ok_or_else(|| "set-option requires an option".to_owned())?;
    let value = positional.get(1).cloned().unwrap_or_default();
    if !unset && positional.get(1).is_none() {
        return Err("set-option requires a value".to_owned());
    }
    Ok(InvocationSpec::plain(Request::SetOption {
        target,
        scope,
        key,
        value,
        unset,
    }))
}

fn parse_show_options(
    arguments: &[String],
    window_command: bool,
) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut global = false;
    let mut window = window_command;
    let mut pane = false;
    let mut show_value = false;
    let mut all = false;
    let mut quiet = false;
    let mut key = None;
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        match option {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-g" => global = true,
            "-w" => window = true,
            "-p" => pane = true,
            "-s" => {}
            "-v" => show_value = true,
            "-A" => all = true,
            "-q" => quiet = true,
            "--" => {
                key = arguments.get(index + 1).cloned();
                break;
            }
            value if value.starts_with('-') && value.len() > 2 => {
                for flag in value[1..].chars() {
                    match flag {
                        'g' => global = true,
                        'w' => window = true,
                        'p' => pane = true,
                        'v' => show_value = true,
                        'A' => all = true,
                        'q' => quiet = true,
                        other => return Err(format!("unknown show-options option: -{other}")),
                    }
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown show-options option: {value}"));
            }
            value => key = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ShowOptions {
        target,
        global,
        window,
        pane,
        value: show_value,
        all,
        quiet,
        key,
    }))
}

fn parse_set_environment(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut remove = false;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-g" | "-s" => {}
            "-r" | "-u" => remove = true,
            "--" => positional.extend_from_slice(&arguments[index + 1..]),
            value if value.starts_with('-') => {
                return Err(format!("unknown set-environment option: {value}"));
            }
            value => positional.push(value.to_owned()),
        }
        if arguments.get(index).is_some_and(|value| value == "--") {
            break;
        }
        index += 1;
    }
    let name = positional
        .first()
        .cloned()
        .ok_or_else(|| "set-environment requires a variable name".to_owned())?;
    let value = positional.get(1).cloned();
    if !remove && value.is_none() {
        return Err("set-environment requires a value".to_owned());
    }
    Ok(InvocationSpec::plain(Request::SetEnvironment {
        name,
        value,
        remove,
    }))
}

fn parse_show_environment(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut format = None;
    let mut name = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-g" | "-s" => {}
            "-F" => {
                index += 1;
                format = Some(argument(arguments, index, "-F")?.to_owned());
            }
            "-t" => {
                index += 1;
                let _ = argument(arguments, index, "-t")?;
            }
            "--" => {
                name = arguments.get(index + 1).cloned();
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown show-environment option: {value}"));
            }
            value => name = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::ShowEnvironment {
        format,
        name,
    }))
}

fn parse_pipe_pane(arguments: &[String]) -> Result<InvocationSpec, String> {
    let mut target = None;
    let mut toggle = false;
    let mut command = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "-o" => toggle = true,
            "-I" | "-O" => {}
            "--" => {
                command = arguments.get(index + 1).cloned();
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown pipe-pane option: {value}"));
            }
            value => command = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(InvocationSpec::plain(Request::PipePane {
        target,
        command,
        toggle,
    }))
}

fn parse_common_tail<F>(arguments: &[String], mut option: F) -> Result<(Vec<String>, bool), String>
where
    F: FnMut(&str, &str) -> Result<(), String>,
{
    let mut command = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let value = &arguments[index];
        if value == "--" {
            command.extend_from_slice(&arguments[index + 1..]);
            return Ok((command, true));
        }
        if matches!(
            value.as_str(),
            "-d" | "-h" | "-v" | "-b" | "-f" | "-k" | "-E" | "-P" | "-a" | "-S"
        ) {
            option(value, "")?;
        } else if matches!(value.as_str(), "-t" | "-n" | "-c" | "-l" | "-F" | "-I") {
            index += 1;
            option(value, argument(arguments, index, value)?)?;
        } else if value.starts_with('-') {
            return Err(format!("unknown option: {value}"));
        } else {
            command.extend_from_slice(&arguments[index..]);
            return Ok((command, false));
        }
        index += 1;
    }
    Ok((command, false))
}

fn target_and_name(arguments: &[String]) -> Result<(Option<String>, String), String> {
    let mut target = None;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "-t" {
            index += 1;
            target = Some(argument(arguments, index, "-t")?.to_owned());
        } else if arguments[index] == "--" {
            positional.extend_from_slice(&arguments[index + 1..]);
            break;
        } else if arguments[index].starts_with('-') {
            return Err(format!("unknown option: {}", arguments[index]));
        } else {
            positional.push(arguments[index].clone());
        }
        index += 1;
    }
    let name = positional
        .first()
        .cloned()
        .ok_or_else(|| "a name is required".to_owned())?;
    Ok((target, name))
}

fn optional_target(arguments: &[String]) -> Result<Option<String>, String> {
    let mut target = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(argument(arguments, index, "-t")?.to_owned());
            }
            "--" => break,
            value if value.starts_with('-') => {
                return Err(format!("unknown target option: {value}"))
            }
            value => target = Some(value.to_owned()),
        }
        index += 1;
    }
    Ok(target)
}

fn required_target(arguments: &[String]) -> Result<String, String> {
    optional_target(arguments)?.ok_or_else(|| "-t target is required".to_owned())
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
            "-X".to_owned(),
            "copy-pipe-and-cancel".to_owned(),
            "-P".to_owned(),
            "-C".to_owned(),
            "-t".to_owned(),
            "session:0".to_owned(),
            "--".to_owned(),
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
        assert!(matches!(next.request, Request::NextWindow { target: None }));
        let previous = parse(&[
            "selectw".to_owned(),
            "-p".to_owned(),
            "-t".to_owned(),
            "S:".to_owned(),
        ])
        .expect("previous");
        assert!(matches!(
            previous.request,
            Request::PreviousWindow { target: Some(target) } if target == "S:"
        ));
        let last = parse(&["last-window".to_owned()]).expect("last");
        assert!(matches!(
            last.request,
            Request::SelectWindow { target } if target == "!"
        ));
        assert!(parse(&["kill-window".to_owned(), "-f".to_owned()]).is_err());
    }

    #[test]
    fn choose_tree_parses_filter_format_sort_and_source_options() {
        let invocation = parse(&[
            "choose-tree".to_owned(),
            "-t".to_owned(),
            "work:1".to_owned(),
            "-f".to_owned(),
            "#{==:#{session_name},work}".to_owned(),
            "-F".to_owned(),
            "#{session_name}:#{window_index}".to_owned(),
            "-O".to_owned(),
            "name".to_owned(),
            "-r".to_owned(),
            "-h".to_owned(),
        ])
        .expect("choose-tree");
        assert!(matches!(
            invocation.request,
            Request::ChooseTree {
                target: Some(target),
                filter: Some(filter),
                format: Some(format),
                sort,
                reverse: true,
                hide_source: true,
                ..
            } if target == "work:1"
                && filter == "#{==:#{session_name},work}"
                && format == "#{session_name}:#{window_index}"
                && sort == "name"
        ));
    }

    #[test]
    fn choose_buffer_and_client_parse_their_mode_options() {
        let buffer = parse(&[
            "choose-buffer".to_owned(),
            "-t".to_owned(),
            "work:0".to_owned(),
            "-f".to_owned(),
            "#{m:*log*,#{buffer_sample}}".to_owned(),
            "-F".to_owned(),
            "#{buffer_name}".to_owned(),
            "-O".to_owned(),
            "name".to_owned(),
            "-r".to_owned(),
        ])
        .expect("choose-buffer");
        assert!(matches!(
            buffer.request,
            Request::ChooseBuffer {
                target: Some(target),
                filter: Some(filter),
                format: Some(format),
                sort,
                reverse: true,
                ..
            } if target == "work:0"
                && filter == "#{m:*log*,#{buffer_sample}}"
                && format == "#{buffer_name}"
                && sort == "name"
        ));

        let client = parse(&[
            "choose-client".to_owned(),
            "-F".to_owned(),
            "#{client_session}".to_owned(),
            "-f".to_owned(),
            "#{==:#{client_session},work}".to_owned(),
        ])
        .expect("choose-client");
        assert!(matches!(
            client.request,
            Request::ChooseClient {
                target: None,
                filter: Some(filter),
                format: Some(format),
                ..
            } if filter == "#{==:#{client_session},work}"
                && format == "#{client_session}"
        ));
    }

    #[test]
    fn display_panes_parses_target_source_zoom_and_selection_command() {
        let invocation = parse(&[
            "display-panes".to_owned(),
            "-k".to_owned(),
            "-Z".to_owned(),
            "-t".to_owned(),
            "%3".to_owned(),
            "-s".to_owned(),
            "work:1".to_owned(),
            "set-option".to_owned(),
            "-g".to_owned(),
            "@picked".to_owned(),
            "%%".to_owned(),
        ])
        .expect("display-panes");
        assert!(matches!(
            invocation.request,
            Request::DisplayPanes {
                target: Some(target),
                source: Some(source),
                no_zoom: true,
                no_mode: false,
                command,
                kill_on_exit: true,
            } if target == "%3"
                && source == "work:1"
                && command == ["set-option", "-g", "@picked", "%%"]
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
        let mouse = parse(&["copy-mode".to_owned(), "-M".to_owned(), "-S".to_owned()])
            .expect("mouse copy mode flags");
        assert!(matches!(
            mouse.request,
            Request::CopyMode {
                mouse_start: true,
                scroll_to_mouse: true,
                ..
            }
        ));
    }

    #[test]
    fn display_panes_accepts_tmux_combined_delay_flags() {
        let invocation = parse(&[
            "display-panes".to_owned(),
            "-Nd".to_owned(),
            "500".to_owned(),
            "set-option".to_owned(),
            "-g".to_owned(),
            "@after".to_owned(),
            "fast".to_owned(),
        ])
        .expect("combined display-panes flags");
        assert!(matches!(
            invocation.request,
            Request::DisplayPanes {
                no_zoom: false,
                no_mode: true,
                command,
                ..
            } if command == ["set-option", "-g", "@after", "fast"]
        ));
    }
}
