use std::io::{self, Read, Write};

use crate::model::Size;

const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_COMMAND_ARGUMENTS: usize = 1024;
pub(crate) const PROTOCOL_VERSION: u32 = 0x544d_000d;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    NewSession {
        name: Option<String>,
        detached: bool,
        attach_existing: bool,
        group_target: Option<String>,
        format: Option<String>,
        window_name: Option<String>,
        command: Vec<String>,
        cwd: Option<String>,
        size: Size,
    },
    Attach {
        target: Option<String>,
        size: Size,
        term: String,
    },
    ListSessions {
        format: Option<String>,
    },
    HasSession {
        target: String,
    },
    KillSession {
        target: Option<String>,
        all: bool,
    },
    NewWindow {
        target: Option<String>,
        name: Option<String>,
        detached: bool,
        empty: bool,
        force: bool,
        format: Option<String>,
        after: bool,
        before: bool,
        select_existing: bool,
        command: Vec<String>,
        cwd: Option<String>,
    },
    SplitWindow {
        target: Option<String>,
        horizontal: bool,
        before: bool,
        detached: bool,
        empty: bool,
        zoom: bool,
        size: Option<String>,
        percentage: Option<String>,
        command: Vec<String>,
        cwd: Option<String>,
        format: Option<String>,
    },
    ListWindows {
        target: Option<String>,
        format: Option<String>,
        filter: Option<String>,
        all: bool,
    },
    ListPanes {
        target: Option<String>,
        format: Option<String>,
        filter: Option<String>,
        scope: PaneListScope,
    },
    SelectWindow {
        target: String,
        step: WindowStep,
    },
    SelectLayout {
        target: Option<String>,
        layout: String,
    },
    NextWindow {
        target: Option<String>,
    },
    PreviousWindow {
        target: Option<String>,
    },
    RenameSession {
        target: Option<String>,
        name: String,
    },
    RenameWindow {
        target: Option<String>,
        name: String,
    },
    KillWindow {
        target: Option<String>,
        all: bool,
    },
    SelectPane {
        target: Option<String>,
        direction: PaneDirection,
        mark: Option<bool>,
        title: Option<String>,
        enabled: Option<bool>,
    },
    KillPane {
        target: Option<String>,
        all: bool,
        filter: Option<String>,
    },
    SendKeys {
        target: Option<String>,
        bytes: Vec<u8>,
        reset: bool,
    },
    SendPrefix {
        target: Option<String>,
    },
    CapturePane {
        target: Option<String>,
        start: Option<i32>,
        end: Option<i32>,
        escape: bool,
        join: bool,
        preserve_trailing: bool,
        print: bool,
        buffer: Option<String>,
    },
    KillServer,
    CopyMode {
        target: Option<String>,
        source: Option<String>,
        exit_on_scroll: bool,
        hide_position: bool,
        page: bool,
        page_down: bool,
        reset: bool,
    },
    CopyModeCommand {
        target: Option<String>,
        action: String,
        repeat: u32,
    },
    ChooseTree {
        target: Option<String>,
    },
    ChooseBuffer {
        target: Option<String>,
    },
    ChooseClient {
        target: Option<String>,
    },
    DisplayPanes {
        target: Option<String>,
        command: Vec<String>,
    },
    DisplayMessage {
        target: Option<String>,
        format: String,
        print: bool,
        literal: bool,
    },
    SetBuffer {
        name: Option<String>,
        append: bool,
        data: Vec<u8>,
        rename: Option<String>,
    },
    ShowBuffer {
        name: Option<String>,
    },
    ListBuffers {
        format: Option<String>,
        filter: Option<String>,
    },
    DeleteBuffer {
        name: Option<String>,
    },
    PasteBuffer {
        target: Option<String>,
        name: Option<String>,
        bracketed: bool,
        separator: Vec<u8>,
        delete: bool,
    },
    LoadBuffer {
        name: Option<String>,
        data: Vec<u8>,
    },
    SaveBuffer {
        name: Option<String>,
        path: String,
        append: bool,
    },
    SetOption {
        target: Option<String>,
        flags: OptionFlags,
        quiet: bool,
        key: String,
        value: Option<String>,
        unset: bool,
    },
    ResizePane {
        target: Option<String>,
        width: Option<String>,
        height: Option<String>,
        up: Option<String>,
        down: Option<String>,
        left: Option<String>,
        right: Option<String>,
        zoom: bool,
    },
    SwapPane {
        source: Option<String>,
        target: Option<String>,
        direction: Option<PaneDirection>,
        detached: bool,
    },
    BreakPane {
        source: Option<String>,
        target: Option<String>,
        name: Option<String>,
        detached: bool,
        format: Option<String>,
    },
    JoinPane {
        source: Option<String>,
        target: Option<String>,
        horizontal: bool,
        before: bool,
        detached: bool,
        size: Option<String>,
        percentage: Option<String>,
    },
    RespawnPane {
        target: Option<String>,
        command: Vec<String>,
        cwd: Option<String>,
        kill: bool,
        empty: bool,
        window: bool,
    },
    ClearHistory {
        target: Option<String>,
    },
    RotateWindow {
        target: Option<String>,
        up: bool,
    },
    SwapWindow {
        source: Option<String>,
        target: Option<String>,
        detached: bool,
    },
    MoveWindow {
        source: Option<String>,
        target: Option<String>,
        after: bool,
        detached: bool,
        force: bool,
        renumber: bool,
    },
    UnlinkWindow {
        target: Option<String>,
        force: bool,
    },
    ShowOptions {
        target: Option<String>,
        flags: OptionFlags,
        value: bool,
        all: bool,
        quiet: bool,
        key: Option<String>,
    },
    SetEnvironment {
        target: Option<String>,
        global: bool,
        name: String,
        value: Option<String>,
        unset: bool,
        clear: bool,
    },
    ShowEnvironment {
        target: Option<String>,
        global: bool,
        shell: bool,
        name: Option<String>,
    },
    PipePane {
        target: Option<String>,
        command: Option<String>,
        toggle: bool,
    },
    ListClients {
        format: Option<String>,
    },
    DetachClient {
        target: Option<String>,
        all: bool,
    },
    SwitchClient {
        client: Option<String>,
        session: String,
    },
    RefreshClient {
        target: Option<String>,
    },
    RunShell {
        command: Option<String>,
        background: bool,
        target: Option<String>,
        cwd: String,
        delay_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneListScope {
    Window,
    Session,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowStep {
    Exact,
    Last,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionFlags {
    pub global: bool,
    pub server: bool,
    pub window: bool,
    pub pane: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
    Next,
    Previous,
    Last,
    Target,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClientMessage {
    Input(Vec<u8>),
    Resize(Size),
    Detach,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ServerMessage {
    Response { ok: bool, body: String },
    Output { status: u8, bytes: Vec<u8> },
    Render(Vec<u8>),
    Restore(Vec<u8>),
    Closed,
}

pub(crate) fn write_version(stream: &mut impl Write) -> io::Result<()> {
    stream.write_all(&PROTOCOL_VERSION.to_le_bytes())
}

pub(crate) fn read_version(stream: &mut impl Read) -> io::Result<u32> {
    let mut version = [0; 4];
    stream.read_exact(&mut version)?;
    Ok(u32::from_le_bytes(version))
}

pub(crate) fn encode_request(
    request: &Request,
    caller_pane: Option<u64>,
) -> io::Result<Vec<u8>> {
    framed(&request_payload(request, caller_pane)?)
}

fn request_payload(request: &Request, caller_pane: Option<u64>) -> io::Result<Vec<u8>> {
    let mut payload = Vec::new();
    match request {
        Request::NewSession {
            name,
            detached,
            attach_existing,
            group_target,
            format,
            window_name,
            command,
            cwd,
            size,
        } => {
            payload.push(1);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *attach_existing);
            put_optional_string(&mut payload, group_target);
            put_optional_string(&mut payload, format);
            put_optional_string(&mut payload, window_name);
            put_strings(&mut payload, command)?;
            put_optional_string(&mut payload, cwd);
            put_size(&mut payload, *size);
        }
        Request::Attach { target, size, term } => {
            payload.push(2);
            put_optional_string(&mut payload, target);
            put_size(&mut payload, *size);
            put_string(&mut payload, term);
        }
        Request::ListSessions { format } => {
            payload.push(3);
            put_optional_string(&mut payload, format);
        }
        Request::HasSession { target } => {
            payload.push(4);
            put_string(&mut payload, target);
        }
        Request::KillSession { target, all } => {
            payload.push(5);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *all);
        }
        Request::NewWindow {
            target,
            name,
            detached,
            empty,
            force,
            format,
            after,
            before,
            select_existing,
            command,
            cwd,
        } => {
            payload.push(6);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *empty);
            put_bool(&mut payload, *force);
            put_optional_string(&mut payload, format);
            put_bool(&mut payload, *after);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *select_existing);
            put_strings(&mut payload, command)?;
            put_optional_string(&mut payload, cwd);
        }
        Request::SplitWindow {
            target,
            horizontal,
            before,
            detached,
            empty,
            zoom,
            size,
            percentage,
            command,
            cwd,
            format,
        } => {
            payload.push(7);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *horizontal);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *empty);
            put_bool(&mut payload, *zoom);
            put_optional_string(&mut payload, size);
            put_optional_string(&mut payload, percentage);
            put_strings(&mut payload, command)?;
            put_optional_string(&mut payload, cwd);
            put_optional_string(&mut payload, format);
        }
        Request::ListWindows {
            target,
            format,
            filter,
            all,
        } => {
            payload.push(8);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, format);
            put_optional_string(&mut payload, filter);
            put_bool(&mut payload, *all);
        }
        Request::ListPanes {
            target,
            format,
            filter,
            scope,
        } => {
            payload.push(9);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, format);
            put_optional_string(&mut payload, filter);
            payload.push(match scope {
                PaneListScope::Window => 1,
                PaneListScope::Session => 2,
                PaneListScope::Server => 3,
            });
        }
        Request::SelectWindow { target, step } => {
            payload.push(10);
            put_string(&mut payload, target);
            payload.push(match step {
                WindowStep::Exact => 1,
                WindowStep::Last => 2,
                WindowStep::Next => 3,
                WindowStep::Previous => 4,
            });
        }
        Request::SendPrefix { target } => {
            payload.push(42);
            put_optional_string(&mut payload, target);
        }
        Request::SelectLayout { target, layout } => {
            payload.push(26);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, layout);
        }
        Request::NextWindow { target } => {
            payload.push(11);
            put_optional_string(&mut payload, target);
        }
        Request::PreviousWindow { target } => {
            payload.push(12);
            put_optional_string(&mut payload, target);
        }
        Request::RenameSession { target, name } => {
            payload.push(13);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, name);
        }
        Request::RenameWindow { target, name } => {
            payload.push(14);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, name);
        }
        Request::KillWindow { target, all } => {
            payload.push(15);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *all);
        }
        Request::SelectPane {
            target,
            direction,
            mark,
            title,
            enabled,
        } => {
            payload.push(16);
            put_optional_string(&mut payload, target);
            payload.push(direction_code(*direction));
            match mark {
                Some(value) => {
                    put_bool(&mut payload, true);
                    put_bool(&mut payload, *value);
                }
                None => put_bool(&mut payload, false),
            }
            put_optional_string(&mut payload, title);
            match enabled {
                Some(value) => {
                    put_bool(&mut payload, true);
                    put_bool(&mut payload, *value);
                }
                None => put_bool(&mut payload, false),
            }
        }
        Request::KillPane {
            target,
            all,
            filter,
        } => {
            payload.push(17);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *all);
            put_optional_string(&mut payload, filter);
        }
        Request::SendKeys {
            target,
            bytes,
            reset,
        } => {
            payload.push(18);
            put_optional_string(&mut payload, target);
            put_bytes(&mut payload, bytes);
            put_bool(&mut payload, *reset);
        }
        Request::CapturePane {
            target,
            start,
            end,
            escape,
            join,
            preserve_trailing,
            print,
            buffer,
        } => {
            payload.push(19);
            put_optional_string(&mut payload, target);
            put_optional_i32(&mut payload, *start);
            put_optional_i32(&mut payload, *end);
            put_bool(&mut payload, *escape);
            put_bool(&mut payload, *join);
            put_bool(&mut payload, *preserve_trailing);
            put_bool(&mut payload, *print);
            put_optional_string(&mut payload, buffer);
        }
        Request::KillServer => payload.push(20),
        Request::CopyMode {
            target,
            source,
            exit_on_scroll,
            hide_position,
            page,
            page_down,
            reset,
        } => {
            payload.push(21);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, source);
            put_bool(&mut payload, *exit_on_scroll);
            put_bool(&mut payload, *hide_position);
            put_bool(&mut payload, *page);
            put_bool(&mut payload, *page_down);
            put_bool(&mut payload, *reset);
        }
        Request::CopyModeCommand {
            target,
            action,
            repeat,
        } => {
            payload.push(22);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, action);
            payload.extend_from_slice(&repeat.to_le_bytes());
        }
        Request::DisplayMessage {
            target,
            format,
            print,
            literal,
        } => {
            payload.push(23);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, format);
            put_bool(&mut payload, *print);
            put_bool(&mut payload, *literal);
        }
        Request::SetBuffer {
            name,
            append,
            data,
            rename,
        } => {
            payload.push(24);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *append);
            put_bytes(&mut payload, data);
            put_optional_string(&mut payload, rename);
        }
        Request::ShowBuffer { name } => {
            payload.push(25);
            put_optional_string(&mut payload, name);
        }
        Request::ListBuffers { format, filter } => {
            payload.push(27);
            put_optional_string(&mut payload, format);
            put_optional_string(&mut payload, filter);
        }
        Request::DeleteBuffer { name } => {
            payload.push(28);
            put_optional_string(&mut payload, name);
        }
        Request::PasteBuffer {
            target,
            name,
            bracketed,
            separator,
            delete,
        } => {
            payload.push(29);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *bracketed);
            put_bytes(&mut payload, separator);
            put_bool(&mut payload, *delete);
        }
        Request::LoadBuffer { name, data } => {
            payload.push(30);
            put_optional_string(&mut payload, name);
            put_bytes(&mut payload, data);
        }
        Request::SaveBuffer { name, path, append } => {
            payload.push(31);
            put_optional_string(&mut payload, name);
            put_string(&mut payload, path);
            put_bool(&mut payload, *append);
        }
        Request::SetOption {
            target,
            flags,
            quiet,
            key,
            value,
            unset,
        } => {
            payload.push(32);
            put_optional_string(&mut payload, target);
            put_option_flags(&mut payload, *flags);
            put_bool(&mut payload, *quiet);
            put_string(&mut payload, key);
            put_optional_string(&mut payload, value);
            put_bool(&mut payload, *unset);
        }
        Request::ResizePane {
            target,
            width,
            height,
            up,
            down,
            left,
            right,
            zoom,
        } => {
            payload.push(33);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, width);
            put_optional_string(&mut payload, height);
            put_optional_string(&mut payload, up);
            put_optional_string(&mut payload, down);
            put_optional_string(&mut payload, left);
            put_optional_string(&mut payload, right);
            put_bool(&mut payload, *zoom);
        }
        Request::SwapPane {
            source,
            target,
            direction,
            detached,
        } => {
            payload.push(34);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            match direction {
                Some(direction) => {
                    put_bool(&mut payload, true);
                    payload.push(direction_code(*direction));
                }
                None => put_bool(&mut payload, false),
            }
            put_bool(&mut payload, *detached);
        }
        Request::BreakPane {
            source,
            target,
            name,
            detached,
            format,
        } => {
            payload.push(35);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *detached);
            put_optional_string(&mut payload, format);
        }
        Request::JoinPane {
            source,
            target,
            horizontal,
            before,
            detached,
            size,
            percentage,
        } => {
            payload.push(36);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *horizontal);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *detached);
            put_optional_string(&mut payload, size);
            put_optional_string(&mut payload, percentage);
        }
        Request::RespawnPane {
            target,
            command,
            cwd,
            kill,
            empty,
            window,
        } => {
            payload.push(37);
            put_optional_string(&mut payload, target);
            put_strings(&mut payload, command)?;
            put_optional_string(&mut payload, cwd);
            put_bool(&mut payload, *kill);
            put_bool(&mut payload, *empty);
            put_bool(&mut payload, *window);
        }
        Request::ClearHistory { target } => {
            payload.push(38);
            put_optional_string(&mut payload, target);
        }
        Request::RotateWindow { target, up } => {
            payload.push(39);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *up);
        }
        Request::SwapWindow {
            source,
            target,
            detached,
        } => {
            payload.push(40);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *detached);
        }
        Request::MoveWindow {
            source,
            target,
            after,
            detached,
            force,
            renumber,
        } => {
            payload.push(41);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *after);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *force);
            put_bool(&mut payload, *renumber);
        }
        Request::UnlinkWindow { target, force } => {
            payload.push(43);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *force);
        }
        Request::ShowOptions {
            target,
            flags,
            value,
            all,
            quiet,
            key,
        } => {
            payload.push(44);
            put_optional_string(&mut payload, target);
            put_option_flags(&mut payload, *flags);
            put_bool(&mut payload, *value);
            put_bool(&mut payload, *all);
            put_bool(&mut payload, *quiet);
            put_optional_string(&mut payload, key);
        }
        Request::SetEnvironment {
            target,
            global,
            name,
            value,
            unset,
            clear,
        } => {
            payload.push(45);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *global);
            put_string(&mut payload, name);
            put_optional_string(&mut payload, value);
            put_bool(&mut payload, *unset);
            put_bool(&mut payload, *clear);
        }
        Request::ShowEnvironment {
            target,
            global,
            shell,
            name,
        } => {
            payload.push(46);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *global);
            put_bool(&mut payload, *shell);
            put_optional_string(&mut payload, name);
        }
        Request::PipePane {
            target,
            command,
            toggle,
        } => {
            payload.push(47);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, command);
            put_bool(&mut payload, *toggle);
        }
        Request::ListClients { format } => {
            payload.push(48);
            put_optional_string(&mut payload, format);
        }
        Request::DetachClient { target, all } => {
            payload.push(49);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *all);
        }
        Request::SwitchClient { client, session } => {
            payload.push(50);
            put_optional_string(&mut payload, client);
            put_string(&mut payload, session);
        }
        Request::RefreshClient { target } => {
            payload.push(51);
            put_optional_string(&mut payload, target);
        }
        Request::RunShell {
            command,
            background,
            target,
            cwd,
            delay_ms,
        } => {
            payload.push(52);
            put_optional_string(&mut payload, command);
            put_bool(&mut payload, *background);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, cwd);
            put_bool(&mut payload, delay_ms.is_some());
            payload.extend_from_slice(&delay_ms.unwrap_or_default().to_le_bytes());
        }
        Request::ChooseTree { target } => {
            payload.push(53);
            put_optional_string(&mut payload, target);
        }
        Request::ChooseBuffer { target } => {
            payload.push(54);
            put_optional_string(&mut payload, target);
        }
        Request::ChooseClient { target } => {
            payload.push(55);
            put_optional_string(&mut payload, target);
        }
        Request::DisplayPanes { target, command } => {
            payload.push(56);
            put_optional_string(&mut payload, target);
            put_strings(&mut payload, command)?;
        }
    }
    put_optional_u64(&mut payload, caller_pane);
    Ok(payload)
}

pub(crate) fn read_request(stream: &mut impl Read) -> io::Result<(Request, Option<u64>)> {
    let payload = read_frame(stream)?;
    let mut decoder = Decoder::new(&payload);
    let request = match decoder.byte()? {
        1 => Request::NewSession {
            name: decoder.optional_string()?,
            detached: decoder.bool()?,
            attach_existing: decoder.bool()?,
            group_target: decoder.optional_string()?,
            format: decoder.optional_string()?,
            window_name: decoder.optional_string()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
            size: decoder.size()?,
        },
        2 => Request::Attach {
            target: decoder.optional_string()?,
            size: decoder.size()?,
            term: decoder.string()?,
        },
        3 => Request::ListSessions {
            format: decoder.optional_string()?,
        },
        4 => Request::HasSession {
            target: decoder.string()?,
        },
        5 => Request::KillSession {
            target: decoder.optional_string()?,
            all: decoder.bool()?,
        },
        6 => Request::NewWindow {
            target: decoder.optional_string()?,
            name: decoder.optional_string()?,
            detached: decoder.bool()?,
            empty: decoder.bool()?,
            force: decoder.bool()?,
            format: decoder.optional_string()?,
            after: decoder.bool()?,
            before: decoder.bool()?,
            select_existing: decoder.bool()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
        },
        7 => Request::SplitWindow {
            target: decoder.optional_string()?,
            horizontal: decoder.bool()?,
            before: decoder.bool()?,
            detached: decoder.bool()?,
            empty: decoder.bool()?,
            zoom: decoder.bool()?,
            size: decoder.optional_string()?,
            percentage: decoder.optional_string()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
            format: decoder.optional_string()?,
        },
        8 => Request::ListWindows {
            target: decoder.optional_string()?,
            format: decoder.optional_string()?,
            filter: decoder.optional_string()?,
            all: decoder.bool()?,
        },
        9 => Request::ListPanes {
            target: decoder.optional_string()?,
            format: decoder.optional_string()?,
            filter: decoder.optional_string()?,
            scope: match decoder.byte()? {
                1 => PaneListScope::Window,
                2 => PaneListScope::Session,
                3 => PaneListScope::Server,
                _ => return Err(invalid_data("unknown pane list scope")),
            },
        },
        10 => Request::SelectWindow {
            target: decoder.string()?,
            step: match decoder.byte()? {
                1 => WindowStep::Exact,
                2 => WindowStep::Last,
                3 => WindowStep::Next,
                4 => WindowStep::Previous,
                _ => return Err(invalid_data("unknown window step")),
            },
        },
        42 => Request::SendPrefix {
            target: decoder.optional_string()?,
        },
        26 => Request::SelectLayout {
            target: decoder.optional_string()?,
            layout: decoder.string()?,
        },
        11 => Request::NextWindow {
            target: decoder.optional_string()?,
        },
        12 => Request::PreviousWindow {
            target: decoder.optional_string()?,
        },
        13 => Request::RenameSession {
            target: decoder.optional_string()?,
            name: decoder.string()?,
        },
        14 => Request::RenameWindow {
            target: decoder.optional_string()?,
            name: decoder.string()?,
        },
        15 => Request::KillWindow {
            target: decoder.optional_string()?,
            all: decoder.bool()?,
        },
        16 => Request::SelectPane {
            target: decoder.optional_string()?,
            direction: decode_direction(decoder.byte()?)?,
            mark: if decoder.bool()? {
                Some(decoder.bool()?)
            } else {
                None
            },
            title: decoder.optional_string()?,
            enabled: if decoder.bool()? {
                Some(decoder.bool()?)
            } else {
                None
            },
        },
        17 => Request::KillPane {
            target: decoder.optional_string()?,
            all: decoder.bool()?,
            filter: decoder.optional_string()?,
        },
        18 => Request::SendKeys {
            target: decoder.optional_string()?,
            bytes: decoder.bytes()?,
            reset: decoder.bool()?,
        },
        19 => Request::CapturePane {
            target: decoder.optional_string()?,
            start: decoder.optional_i32()?,
            end: decoder.optional_i32()?,
            escape: decoder.bool()?,
            join: decoder.bool()?,
            preserve_trailing: decoder.bool()?,
            print: decoder.bool()?,
            buffer: decoder.optional_string()?,
        },
        20 => Request::KillServer,
        21 => Request::CopyMode {
            target: decoder.optional_string()?,
            source: decoder.optional_string()?,
            exit_on_scroll: decoder.bool()?,
            hide_position: decoder.bool()?,
            page: decoder.bool()?,
            page_down: decoder.bool()?,
            reset: decoder.bool()?,
        },
        22 => Request::CopyModeCommand {
            target: decoder.optional_string()?,
            action: decoder.string()?,
            repeat: decoder.u32()?,
        },
        23 => Request::DisplayMessage {
            target: decoder.optional_string()?,
            format: decoder.string()?,
            print: decoder.bool()?,
            literal: decoder.bool()?,
        },
        24 => Request::SetBuffer {
            name: decoder.optional_string()?,
            append: decoder.bool()?,
            data: decoder.bytes()?,
            rename: decoder.optional_string()?,
        },
        25 => Request::ShowBuffer {
            name: decoder.optional_string()?,
        },
        27 => Request::ListBuffers {
            format: decoder.optional_string()?,
            filter: decoder.optional_string()?,
        },
        28 => Request::DeleteBuffer {
            name: decoder.optional_string()?,
        },
        29 => Request::PasteBuffer {
            target: decoder.optional_string()?,
            name: decoder.optional_string()?,
            bracketed: decoder.bool()?,
            separator: decoder.bytes()?,
            delete: decoder.bool()?,
        },
        30 => Request::LoadBuffer {
            name: decoder.optional_string()?,
            data: decoder.bytes()?,
        },
        31 => Request::SaveBuffer {
            name: decoder.optional_string()?,
            path: decoder.string()?,
            append: decoder.bool()?,
        },
        32 => Request::SetOption {
            target: decoder.optional_string()?,
            flags: decoder.option_flags()?,
            quiet: decoder.bool()?,
            key: decoder.string()?,
            value: decoder.optional_string()?,
            unset: decoder.bool()?,
        },
        33 => Request::ResizePane {
            target: decoder.optional_string()?,
            width: decoder.optional_string()?,
            height: decoder.optional_string()?,
            up: decoder.optional_string()?,
            down: decoder.optional_string()?,
            left: decoder.optional_string()?,
            right: decoder.optional_string()?,
            zoom: decoder.bool()?,
        },
        34 => Request::SwapPane {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            direction: if decoder.bool()? {
                Some(decode_direction(decoder.byte()?)?)
            } else {
                None
            },
            detached: decoder.bool()?,
        },
        35 => Request::BreakPane {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            name: decoder.optional_string()?,
            detached: decoder.bool()?,
            format: decoder.optional_string()?,
        },
        36 => Request::JoinPane {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            horizontal: decoder.bool()?,
            before: decoder.bool()?,
            detached: decoder.bool()?,
            size: decoder.optional_string()?,
            percentage: decoder.optional_string()?,
        },
        37 => Request::RespawnPane {
            target: decoder.optional_string()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
            kill: decoder.bool()?,
            empty: decoder.bool()?,
            window: decoder.bool()?,
        },
        38 => Request::ClearHistory {
            target: decoder.optional_string()?,
        },
        39 => Request::RotateWindow {
            target: decoder.optional_string()?,
            up: decoder.bool()?,
        },
        40 => Request::SwapWindow {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            detached: decoder.bool()?,
        },
        41 => Request::MoveWindow {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            after: decoder.bool()?,
            detached: decoder.bool()?,
            force: decoder.bool()?,
            renumber: decoder.bool()?,
        },
        43 => Request::UnlinkWindow {
            target: decoder.optional_string()?,
            force: decoder.bool()?,
        },
        44 => Request::ShowOptions {
            target: decoder.optional_string()?,
            flags: decoder.option_flags()?,
            value: decoder.bool()?,
            all: decoder.bool()?,
            quiet: decoder.bool()?,
            key: decoder.optional_string()?,
        },
        45 => Request::SetEnvironment {
            target: decoder.optional_string()?,
            global: decoder.bool()?,
            name: decoder.string()?,
            value: decoder.optional_string()?,
            unset: decoder.bool()?,
            clear: decoder.bool()?,
        },
        46 => Request::ShowEnvironment {
            target: decoder.optional_string()?,
            global: decoder.bool()?,
            shell: decoder.bool()?,
            name: decoder.optional_string()?,
        },
        47 => Request::PipePane {
            target: decoder.optional_string()?,
            command: decoder.optional_string()?,
            toggle: decoder.bool()?,
        },
        48 => Request::ListClients {
            format: decoder.optional_string()?,
        },
        49 => Request::DetachClient {
            target: decoder.optional_string()?,
            all: decoder.bool()?,
        },
        50 => Request::SwitchClient {
            client: decoder.optional_string()?,
            session: decoder.string()?,
        },
        51 => Request::RefreshClient {
            target: decoder.optional_string()?,
        },
        52 => {
            let command = decoder.optional_string()?;
            let background = decoder.bool()?;
            let target = decoder.optional_string()?;
            let cwd = decoder.string()?;
            let delayed = decoder.bool()?;
            let millis = decoder.u32()?;
            Request::RunShell {
                command,
                background,
                target,
                cwd,
                delay_ms: delayed.then_some(millis),
            }
        }
        53 => Request::ChooseTree {
            target: decoder.optional_string()?,
        },
        54 => Request::ChooseBuffer {
            target: decoder.optional_string()?,
        },
        55 => Request::ChooseClient {
            target: decoder.optional_string()?,
        },
        56 => Request::DisplayPanes {
            target: decoder.optional_string()?,
            command: decoder.strings()?,
        },
        _ => return Err(invalid_data("unknown request tag")),
    };
    let caller_pane = decoder.optional_u64()?;
    decoder.finish()?;
    Ok((request, caller_pane))
}

pub(crate) fn write_server_message(
    stream: &mut impl Write,
    message: &ServerMessage,
) -> io::Result<()> {
    let mut payload = Vec::new();
    match message {
        ServerMessage::Response { ok, body } => {
            payload.push(1);
            put_bool(&mut payload, *ok);
            put_string(&mut payload, body);
        }
        ServerMessage::Output { status, bytes } => {
            payload.push(5);
            payload.push(*status);
            put_bytes(&mut payload, bytes);
        }
        ServerMessage::Render(bytes) => {
            payload.push(2);
            put_bytes(&mut payload, bytes);
        }
        ServerMessage::Restore(bytes) => {
            payload.push(4);
            put_bytes(&mut payload, bytes);
        }
        ServerMessage::Closed => payload.push(3),
    }
    write_frame(stream, &payload)
}

pub(crate) fn read_server_message(stream: &mut impl Read) -> io::Result<ServerMessage> {
    let payload = read_frame(stream)?;
    let mut decoder = Decoder::new(&payload);
    let message = match decoder.byte()? {
        1 => ServerMessage::Response {
            ok: decoder.bool()?,
            body: decoder.string()?,
        },
        2 => ServerMessage::Render(decoder.bytes()?),
        5 => ServerMessage::Output {
            status: decoder.byte()?,
            bytes: decoder.bytes()?,
        },
        4 => ServerMessage::Restore(decoder.bytes()?),
        3 => ServerMessage::Closed,
        _ => return Err(invalid_data("unknown server message tag")),
    };
    decoder.finish()?;
    Ok(message)
}

pub(crate) fn write_client_message(
    stream: &mut impl Write,
    message: &ClientMessage,
) -> io::Result<()> {
    let mut payload = Vec::new();
    match message {
        ClientMessage::Input(bytes) => {
            payload.push(1);
            put_bytes(&mut payload, bytes);
        }
        ClientMessage::Resize(size) => {
            payload.push(2);
            put_size(&mut payload, *size);
        }
        ClientMessage::Detach => payload.push(3),
    }
    write_frame(stream, &payload)
}

pub(crate) fn read_client_message(stream: &mut impl Read) -> io::Result<ClientMessage> {
    let payload = read_frame(stream)?;
    let mut decoder = Decoder::new(&payload);
    let message = match decoder.byte()? {
        1 => ClientMessage::Input(decoder.bytes()?),
        2 => ClientMessage::Resize(decoder.size()?),
        3 => ClientMessage::Detach,
        _ => return Err(invalid_data("unknown client message tag")),
    };
    decoder.finish()?;
    Ok(message)
}

fn framed(payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() > MAX_FRAME {
        return Err(invalid_data("frame is too large"));
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

fn write_frame(stream: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    stream.write_all(&framed(payload)?)
}

fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(invalid_data("invalid frame length"));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload)?;
    Ok(payload)
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    put_bytes(output, value.as_bytes());
}

fn put_optional_string(output: &mut Vec<u8>, value: &Option<String>) {
    match value {
        Some(value) => {
            output.push(1);
            put_string(output, value);
        }
        None => output.push(0),
    }
}

fn put_strings(output: &mut Vec<u8>, values: &[String]) -> io::Result<()> {
    if values.len() > MAX_COMMAND_ARGUMENTS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "too many arguments: {} (limit {MAX_COMMAND_ARGUMENTS})",
                values.len()
            ),
        ));
    }
    output.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        put_string(output, value);
    }
    Ok(())
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn put_bool(output: &mut Vec<u8>, value: bool) {
    output.push(u8::from(value));
}

fn put_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            put_bool(output, true);
            output.extend_from_slice(&value.to_le_bytes());
        }
        None => put_bool(output, false),
    }
}

fn put_optional_i32(output: &mut Vec<u8>, value: Option<i32>) {
    match value {
        Some(value) => {
            put_bool(output, true);
            output.extend_from_slice(&value.to_le_bytes());
        }
        None => put_bool(output, false),
    }
}

fn put_size(output: &mut Vec<u8>, size: Size) {
    output.extend_from_slice(&size.cols().to_le_bytes());
    output.extend_from_slice(&size.rows().to_le_bytes());
}

fn direction_code(direction: PaneDirection) -> u8 {
    match direction {
        PaneDirection::Left => 1,
        PaneDirection::Right => 2,
        PaneDirection::Up => 3,
        PaneDirection::Down => 4,
        PaneDirection::Next => 5,
        PaneDirection::Previous => 6,
        PaneDirection::Last => 7,
        PaneDirection::Target => 8,
    }
}

fn decode_direction(code: u8) -> io::Result<PaneDirection> {
    match code {
        1 => Ok(PaneDirection::Left),
        2 => Ok(PaneDirection::Right),
        3 => Ok(PaneDirection::Up),
        4 => Ok(PaneDirection::Down),
        5 => Ok(PaneDirection::Next),
        6 => Ok(PaneDirection::Previous),
        7 => Ok(PaneDirection::Last),
        8 => Ok(PaneDirection::Target),
        _ => Err(invalid_data("unknown pane direction")),
    }
}

fn put_option_flags(payload: &mut Vec<u8>, flags: OptionFlags) {
    put_bool(payload, flags.global);
    put_bool(payload, flags.server);
    put_bool(payload, flags.window);
    put_bool(payload, flags.pane);
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> io::Result<u8> {
        let byte = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or_else(|| invalid_data("truncated frame"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn bool(&mut self) -> io::Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid_data("invalid boolean")),
        }
    }

    fn option_flags(&mut self) -> io::Result<OptionFlags> {
        Ok(OptionFlags {
            global: self.bool()?,
            server: self.bool()?,
            window: self.bool()?,
            pane: self.bool()?,
        })
    }

    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = self.u32()? as usize;
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| invalid_data("length overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid_data("truncated bytes"));
        }
        let value = self.bytes[self.offset..end].to_vec();
        self.offset = end;
        Ok(value)
    }

    fn string(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?).map_err(|_| invalid_data("invalid UTF-8 string"))
    }

    fn optional_string(&mut self) -> io::Result<Option<String>> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.string()?)),
            _ => Err(invalid_data("invalid optional string")),
        }
    }

    fn strings(&mut self) -> io::Result<Vec<String>> {
        let count = self.u32()? as usize;
        if count > MAX_COMMAND_ARGUMENTS {
            return Err(invalid_data("too many command arguments"));
        }
        (0..count).map(|_| self.string()).collect()
    }

    fn size(&mut self) -> io::Result<Size> {
        Ok(Size::new(self.u16()?, self.u16()?))
    }

    fn u16(&mut self) -> io::Result<u16> {
        let end = self
            .offset
            .checked_add(2)
            .ok_or_else(|| invalid_data("length overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid_data("truncated integer"));
        }
        let value = u16::from_le_bytes([self.bytes[self.offset], self.bytes[self.offset + 1]]);
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> io::Result<u32> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or_else(|| invalid_data("length overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid_data("truncated integer"));
        }
        let value = u32::from_le_bytes([
            self.bytes[self.offset],
            self.bytes[self.offset + 1],
            self.bytes[self.offset + 2],
            self.bytes[self.offset + 3],
        ]);
        self.offset = end;
        Ok(value)
    }

    fn optional_u64(&mut self) -> io::Result<Option<u64>> {
        if !self.bool()? {
            return Ok(None);
        }
        let mut value = [0; 8];
        for byte in &mut value {
            *byte = self.byte()?;
        }
        Ok(Some(u64::from_le_bytes(value)))
    }

    fn optional_i32(&mut self) -> io::Result<Option<i32>> {
        if !self.bool()? {
            return Ok(None);
        }
        Ok(Some(i32::from_le_bytes([
            self.byte()?,
            self.byte()?,
            self.byte()?,
            self.byte()?,
        ])))
    }

    fn finish(&self) -> io::Result<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid_data("trailing frame data"))
        }
    }
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_preserves_command_arguments_and_target() {
        let request = Request::SplitWindow {
            target: Some("work:1.2".to_owned()),
            horizontal: true,
            before: false,
            detached: false,
            empty: false,
            zoom: false,
            size: None,
            percentage: None,
            command: vec!["printf".to_owned(), "a\tb".to_owned()],
            cwd: Some("/tmp".to_owned()),
            format: None,
        };
        let bytes = encode_request(&request, None).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
            Request::SplitWindow {
                horizontal: true,
                ..
            }
        ));
    }

    #[test]
    fn caller_pane_round_trips_beside_the_request() {
        let request = Request::ListSessions { format: None };
        for caller in [None, Some(0), Some(u64::MAX)] {
            let frame = encode_request(&request, caller).expect("encode");
            let (decoded, decoded_caller) =
                read_request(&mut std::io::Cursor::new(frame)).expect("decode");
            assert_eq!(decoded, request);
            assert_eq!(decoded_caller, caller);
        }
    }

    #[test]
    fn chooser_requests_round_trip_their_target() {
        for request in [
            Request::ChooseTree { target: Some("work:1".to_owned()) },
            Request::ChooseBuffer { target: Some("work:0".to_owned()) },
            Request::ChooseClient { target: None },
        ] {
            let bytes = encode_request(&request, None).expect("encode");
            let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
            let decoded = read_request_from_payload(&mut cursor).expect("decode");
            assert_eq!(decoded, request);
        }
    }

    #[test]
    fn paste_buffer_request_round_trip_preserves_bracketed_mode() {
        let request = Request::PasteBuffer {
            target: Some("work:0.1".to_owned()),
            name: Some("paste".to_owned()),
            bracketed: true,
            separator: b"|".to_vec(),
            delete: true,
        };
        let bytes = encode_request(&request, None).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
            Request::PasteBuffer {
                target: Some(target),
                name: Some(name),
                bracketed: true,
                separator,
                delete: true,
                ..
            } if target == "work:0.1" && name == "paste" && separator == b"|"
        ));
    }

    #[test]
    fn display_panes_request_round_trip_preserves_selection_contract() {
        let request = Request::DisplayPanes {
            target: Some("%3".to_owned()),
            command: vec![
                "set-option".to_owned(),
                "-g".to_owned(),
                "@picked".to_owned(),
                "%%".to_owned(),
            ],
        };
        let bytes = encode_request(&request, None).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
            Request::DisplayPanes {
                target: Some(target),
                command,
            } if target == "%3"
                && command == ["set-option", "-g", "@picked", "%%"]
        ));
    }

    #[test]
    fn copy_mode_request_round_trip_preserves_page_and_reset_options() {
        for request in [
            Request::CopyMode {
                target: Some("work:0".to_owned()),
                source: Some("work:1.2".to_owned()),
                exit_on_scroll: true,
                hide_position: true,
                page: false,
                page_down: true,
                reset: false,
            },
            Request::CopyMode {
                target: Some("work:0".to_owned()),
                source: None,
                exit_on_scroll: false,
                hide_position: false,
                page: false,
                page_down: false,
                reset: true,
            },
        ] {
            let bytes = encode_request(&request, None).expect("encode");
            let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
            let decoded = read_request_from_payload(&mut cursor).expect("decode");
            match (request, decoded) {
                (
                    Request::CopyMode {
                        source: Some(source),
                        ..
                    },
                    Request::CopyMode {
                        source: Some(decoded_source),
                        ..
                    },
                ) => assert_eq!(source, decoded_source),
                (
                    Request::CopyMode { source: None, .. },
                    Request::CopyMode { source: None, .. },
                ) => {}
                _ => panic!("copy-mode source did not round-trip"),
            }
        }
    }

    fn read_request_from_payload(cursor: &mut std::io::Cursor<Vec<u8>>) -> io::Result<Request> {
        let payload = cursor.get_ref().clone();
        let mut framed = Vec::new();
        framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        framed.extend_from_slice(&payload);
        read_request(&mut std::io::Cursor::new(framed)).map(|(request, _)| request)
    }

    const REQUEST_VARIANTS: usize = 56;

    trait Source {
        fn flag(&mut self) -> bool;
        fn opt_flag(&mut self) -> Option<bool>;
        fn text(&mut self) -> String;
        fn opt_text(&mut self) -> Option<String>;
        fn texts(&mut self) -> Vec<String>;
        fn blob(&mut self) -> Vec<u8>;
        fn number(&mut self) -> u32;
        fn signed(&mut self) -> i32;
        fn opt_signed(&mut self) -> Option<i32>;
        fn size(&mut self) -> Size;
        fn direction(&mut self) -> PaneDirection;
        fn opt_direction(&mut self) -> Option<PaneDirection>;
        fn option_flags(&mut self) -> OptionFlags;
        fn pane_list_scope(&mut self) -> PaneListScope;
        fn window_step(&mut self) -> WindowStep;
    }

    const DIRECTIONS: [PaneDirection; 8] = [
        PaneDirection::Left,
        PaneDirection::Right,
        PaneDirection::Up,
        PaneDirection::Down,
        PaneDirection::Next,
        PaneDirection::Previous,
        PaneDirection::Last,
        PaneDirection::Target,
    ];

    const PANE_LIST_SCOPES: [PaneListScope; 3] = [
        PaneListScope::Window,
        PaneListScope::Session,
        PaneListScope::Server,
    ];

    const WINDOW_STEPS: [WindowStep; 4] = [
        WindowStep::Exact,
        WindowStep::Last,
        WindowStep::Next,
        WindowStep::Previous,
    ];

    struct Canonical {
        calls: usize,
    }

    impl Canonical {
        fn toggle(&mut self) -> bool {
            self.calls += 1;
            !self.calls.is_multiple_of(3)
        }
    }

    impl Source for Canonical {
        fn flag(&mut self) -> bool {
            self.toggle()
        }
        fn opt_flag(&mut self) -> Option<bool> {
            let present = self.toggle();
            present.then(|| self.toggle())
        }
        fn text(&mut self) -> String {
            self.calls += 1;
            format!("t{}", self.calls)
        }
        fn opt_text(&mut self) -> Option<String> {
            self.toggle().then(|| self.text())
        }
        fn texts(&mut self) -> Vec<String> {
            vec![self.text(), "\u{e9}\t".to_owned()]
        }
        fn blob(&mut self) -> Vec<u8> {
            self.calls += 1;
            vec![0, self.calls as u8, 255]
        }
        fn number(&mut self) -> u32 {
            self.calls += 1;
            0x0100_0000 + self.calls as u32
        }
        fn signed(&mut self) -> i32 {
            self.calls += 1;
            -(self.calls as i32)
        }
        fn opt_signed(&mut self) -> Option<i32> {
            self.toggle().then(|| self.signed())
        }
        fn size(&mut self) -> Size {
            Size::new(80, 24)
        }
        fn direction(&mut self) -> PaneDirection {
            self.calls += 1;
            DIRECTIONS[self.calls % DIRECTIONS.len()]
        }
        fn opt_direction(&mut self) -> Option<PaneDirection> {
            self.toggle().then(|| self.direction())
        }
        fn option_flags(&mut self) -> OptionFlags {
            self.calls += 1;
            OptionFlags {
                global: self.toggle(),
                server: self.toggle(),
                window: self.toggle(),
                pane: self.toggle(),
            }
        }
        fn pane_list_scope(&mut self) -> PaneListScope {
            self.calls += 1;
            PANE_LIST_SCOPES[self.calls % PANE_LIST_SCOPES.len()]
        }
        fn window_step(&mut self) -> WindowStep {
            self.calls += 1;
            WINDOW_STEPS[self.calls % WINDOW_STEPS.len()]
        }
    }

    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, bound: usize) -> usize {
            (self.next() % bound as u64) as usize
        }
    }

    impl Source for Random {
        fn flag(&mut self) -> bool {
            self.below(2) == 1
        }
        fn opt_flag(&mut self) -> Option<bool> {
            self.flag().then(|| self.flag())
        }
        fn text(&mut self) -> String {
            const POOL: [char; 12] = ['a', 'Z', '0', ' ', '\t', '\n', '#', '{', '\u{e9}', '\u{4e2d}', '\u{1f600}', '\0'];
            (0..self.below(40)).map(|_| POOL[self.below(POOL.len())]).collect()
        }
        fn opt_text(&mut self) -> Option<String> {
            self.flag().then(|| self.text())
        }
        fn texts(&mut self) -> Vec<String> {
            (0..self.below(7)).map(|_| self.text()).collect()
        }
        fn blob(&mut self) -> Vec<u8> {
            (0..self.below(64)).map(|_| self.next() as u8).collect()
        }
        fn number(&mut self) -> u32 {
            self.next() as u32
        }
        fn signed(&mut self) -> i32 {
            self.next() as i32
        }
        fn opt_signed(&mut self) -> Option<i32> {
            self.flag().then(|| self.signed())
        }
        fn size(&mut self) -> Size {
            Size::new(self.next() as u16, self.next() as u16)
        }
        fn direction(&mut self) -> PaneDirection {
            DIRECTIONS[self.below(DIRECTIONS.len())]
        }
        fn opt_direction(&mut self) -> Option<PaneDirection> {
            self.flag().then(|| self.direction())
        }
        fn option_flags(&mut self) -> OptionFlags {
            OptionFlags {
                global: self.flag(),
                server: self.flag(),
                window: self.flag(),
                pane: self.flag(),
            }
        }
        fn pane_list_scope(&mut self) -> PaneListScope {
            PANE_LIST_SCOPES[self.below(PANE_LIST_SCOPES.len())]
        }
        fn window_step(&mut self) -> WindowStep {
            WINDOW_STEPS[self.below(WINDOW_STEPS.len())]
        }
    }

    fn build(variant: usize, source: &mut impl Source) -> Request {
        match variant {
        0 => Request::NewSession {
                name: source.opt_text(),
                detached: source.flag(),
                attach_existing: source.flag(),
                group_target: source.opt_text(),
                format: source.opt_text(),
                window_name: source.opt_text(),
                command: source.texts(),
                cwd: source.opt_text(),
                size: source.size(),
        },
        1 => Request::Attach {
                target: source.opt_text(),
                size: source.size(),
                term: source.text(),
        },
        2 => Request::ListSessions {
                format: source.opt_text(),
        },
        3 => Request::HasSession {
                target: source.text(),
        },
        4 => Request::KillSession {
                target: source.opt_text(),
                all: source.flag(),
        },
        5 => Request::NewWindow {
                target: source.opt_text(),
                name: source.opt_text(),
                detached: source.flag(),
                empty: source.flag(),
                force: source.flag(),
                format: source.opt_text(),
                after: source.flag(),
                before: source.flag(),
                select_existing: source.flag(),
                command: source.texts(),
                cwd: source.opt_text(),
        },
        6 => Request::SplitWindow {
                target: source.opt_text(),
                horizontal: source.flag(),
                before: source.flag(),
                detached: source.flag(),
                empty: source.flag(),
                zoom: source.flag(),
                size: source.opt_text(),
                percentage: source.opt_text(),
                command: source.texts(),
                cwd: source.opt_text(),
                format: source.opt_text(),
        },
        7 => Request::ListWindows {
                target: source.opt_text(),
                format: source.opt_text(),
                filter: source.opt_text(),
                all: source.flag(),
        },
        8 => Request::ListPanes {
                target: source.opt_text(),
                format: source.opt_text(),
                filter: source.opt_text(),
                scope: source.pane_list_scope(),
        },
        9 => Request::SelectWindow {
                target: source.text(),
                step: source.window_step(),
        },
        10 => Request::SelectLayout {
                target: source.opt_text(),
                layout: source.text(),
        },
        11 => Request::NextWindow {
                target: source.opt_text(),
        },
        12 => Request::PreviousWindow {
                target: source.opt_text(),
        },
        13 => Request::RenameSession {
                target: source.opt_text(),
                name: source.text(),
        },
        14 => Request::RenameWindow {
                target: source.opt_text(),
                name: source.text(),
        },
        15 => Request::KillWindow {
                target: source.opt_text(),
                all: source.flag(),
        },
        16 => Request::SelectPane {
                target: source.opt_text(),
                direction: source.direction(),
                mark: source.opt_flag(),
                title: source.opt_text(),
                enabled: source.opt_flag(),
        },
        17 => Request::KillPane {
                target: source.opt_text(),
                all: source.flag(),
                filter: source.opt_text(),
        },
        18 => Request::SendKeys {
                target: source.opt_text(),
                bytes: source.blob(),
                reset: source.flag(),
        },
        19 => Request::SendPrefix {
                target: source.opt_text(),
        },
        20 => Request::CapturePane {
                target: source.opt_text(),
                start: source.opt_signed(),
                end: source.opt_signed(),
                escape: source.flag(),
                join: source.flag(),
                preserve_trailing: source.flag(),
                print: source.flag(),
                buffer: source.opt_text(),
        },
        21 => Request::KillServer,
        22 => Request::CopyMode {
                target: source.opt_text(),
                source: source.opt_text(),
                exit_on_scroll: source.flag(),
                hide_position: source.flag(),
                page: source.flag(),
                page_down: source.flag(),
                reset: source.flag(),
        },
        23 => Request::CopyModeCommand {
                target: source.opt_text(),
                action: source.text(),
                repeat: source.number(),
        },
        24 => Request::ChooseTree {
                target: source.opt_text(),
        },
        25 => Request::ChooseBuffer {
                target: source.opt_text(),
        },
        26 => Request::ChooseClient {
                target: source.opt_text(),
        },
        27 => Request::DisplayPanes {
                target: source.opt_text(),
                command: source.texts(),
        },
        28 => Request::DisplayMessage {
                target: source.opt_text(),
                format: source.text(),
                print: source.flag(),
                literal: source.flag(),
        },
        29 => Request::SetBuffer {
                name: source.opt_text(),
                append: source.flag(),
                data: source.blob(),
                rename: source.opt_text(),
        },
        30 => Request::ShowBuffer {
                name: source.opt_text(),
        },
        31 => Request::ListBuffers {
                format: source.opt_text(),
                filter: source.opt_text(),
        },
        32 => Request::DeleteBuffer {
                name: source.opt_text(),
        },
        33 => Request::PasteBuffer {
                target: source.opt_text(),
                name: source.opt_text(),
                bracketed: source.flag(),
                separator: source.blob(),
                delete: source.flag(),
        },
        34 => Request::LoadBuffer {
                name: source.opt_text(),
                data: source.blob(),
        },
        35 => Request::SaveBuffer {
                name: source.opt_text(),
                path: source.text(),
                append: source.flag(),
        },
        36 => Request::SetOption {
                target: source.opt_text(),
                flags: source.option_flags(),
                quiet: source.flag(),
                key: source.text(),
                value: source.opt_text(),
                unset: source.flag(),
        },
        37 => Request::ResizePane {
                target: source.opt_text(),
                width: source.opt_text(),
                height: source.opt_text(),
                up: source.opt_text(),
                down: source.opt_text(),
                left: source.opt_text(),
                right: source.opt_text(),
                zoom: source.flag(),
        },
        38 => Request::SwapPane {
                source: source.opt_text(),
                target: source.opt_text(),
                direction: source.opt_direction(),
                detached: source.flag(),
        },
        39 => Request::BreakPane {
                source: source.opt_text(),
                target: source.opt_text(),
                name: source.opt_text(),
                detached: source.flag(),
                format: source.opt_text(),
        },
        40 => Request::JoinPane {
                source: source.opt_text(),
                target: source.opt_text(),
                horizontal: source.flag(),
                before: source.flag(),
                detached: source.flag(),
                size: source.opt_text(),
                percentage: source.opt_text(),
        },
        41 => Request::RespawnPane {
                target: source.opt_text(),
                command: source.texts(),
                cwd: source.opt_text(),
                kill: source.flag(),
                empty: source.flag(),
                window: source.flag(),
        },
        42 => Request::ClearHistory {
                target: source.opt_text(),
        },
        44 => Request::RotateWindow {
                target: source.opt_text(),
                up: source.flag(),
        },
        45 => Request::SwapWindow {
                source: source.opt_text(),
                target: source.opt_text(),
                detached: source.flag(),
        },
        46 => Request::MoveWindow {
                source: source.opt_text(),
                target: source.opt_text(),
                after: source.flag(),
                detached: source.flag(),
                force: source.flag(),
                renumber: source.flag(),
        },
        48 => Request::UnlinkWindow {
                target: source.opt_text(),
                force: source.flag(),
        },
        49 => Request::ShowOptions {
                target: source.opt_text(),
                flags: source.option_flags(),
                value: source.flag(),
                all: source.flag(),
                quiet: source.flag(),
                key: source.opt_text(),
        },
        50 => Request::SetEnvironment {
                target: source.opt_text(),
                global: source.flag(),
                name: source.text(),
                value: source.opt_text(),
                unset: source.flag(),
                clear: source.flag(),
        },
        51 => Request::ShowEnvironment {
                target: source.opt_text(),
                global: source.flag(),
                shell: source.flag(),
                name: source.opt_text(),
        },
        52 => Request::PipePane {
                target: source.opt_text(),
                command: source.opt_text(),
                toggle: source.flag(),
        },
        53 => Request::ListClients {
                format: source.opt_text(),
        },
        54 => Request::DetachClient {
                target: source.opt_text(),
                all: source.flag(),
        },
        55 => Request::SwitchClient {
                client: source.opt_text(),
                session: source.text(),
        },
        43 => Request::RefreshClient {
                target: source.opt_text(),
        },
        47 => Request::RunShell {
                command: source.opt_text(),
                background: source.flag(),
                target: source.opt_text(),
                cwd: source.text(),
                delay_ms: source.flag().then(|| source.number()),
        },
        _ => unreachable!("request variant index out of range"),
        }
    }

    fn decode_frame(frame: &[u8]) -> io::Result<Request> {
        read_request(&mut std::io::Cursor::new(frame.to_vec())).map(|(request, _)| request)
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn random_requests_inside_the_domain_round_trip() {
        let mut random = Random(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200 {
            for variant in 0..REQUEST_VARIANTS {
                let request = build(variant, &mut random);
                let frame = encode_request(&request, None).expect("encode");
                assert_eq!(decode_frame(&frame).expect("decode"), request);
            }
        }
    }

    #[test]
    fn random_messages_round_trip() {
        let mut random = Random(0x2545_f491_4f6c_dd1d);
        for _ in 0..200 {
            let client = match random.below(3) {
                0 => ClientMessage::Input(random.blob()),
                1 => ClientMessage::Resize(random.size()),
                _ => ClientMessage::Detach,
            };
            let mut bytes = Vec::new();
            write_client_message(&mut bytes, &client).expect("encode");
            assert_eq!(
                read_client_message(&mut std::io::Cursor::new(bytes)).expect("decode"),
                client
            );
            let server = match random.below(4) {
                0 => ServerMessage::Response {
                    ok: random.flag(),
                    body: random.text(),
                },
                1 => ServerMessage::Render(random.blob()),
                2 => ServerMessage::Restore(random.blob()),
                _ => ServerMessage::Closed,
            };
            let mut bytes = Vec::new();
            write_server_message(&mut bytes, &server).expect("encode");
            assert_eq!(
                read_server_message(&mut std::io::Cursor::new(bytes)).expect("decode"),
                server
            );
        }
    }

    fn new_window_with_commands(count: usize) -> Request {
        Request::NewWindow {
            target: None,
            name: None,
            detached: false,
            empty: false,
            force: false,
            format: None,
            after: false,
            before: false,
            select_existing: false,
            command: (0..count).map(|index| format!("arg{index}")).collect(),
            cwd: None,
        }
    }

    #[test]
    fn command_arguments_at_the_limit_round_trip_and_beyond_it_are_refused() {
        let at_limit = new_window_with_commands(MAX_COMMAND_ARGUMENTS);
        let frame = encode_request(&at_limit, None).expect("1024 arguments encode");
        assert_eq!(decode_frame(&frame).expect("decode"), at_limit);

        let error = encode_request(&new_window_with_commands(MAX_COMMAND_ARGUMENTS + 1), None)
            .expect_err("1025 arguments are refused");
        assert_eq!(error.to_string(), "too many arguments: 1025 (limit 1024)");
    }

    #[test]
    fn oversized_frames_are_refused_by_encoder_and_decoder() {
        let request = Request::LoadBuffer {
            name: None,
            data: vec![0; MAX_FRAME],
        };
        assert_eq!(
            encode_request(&request, None).expect_err("oversized").to_string(),
            "frame is too large"
        );
        let mut header = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
        header.push(1);
        assert!(decode_frame(&header).is_err());
        assert!(decode_frame(&0_u32.to_le_bytes()).is_err());
    }

    #[test]
    fn version_preamble_is_never_a_valid_frame_length_for_an_older_daemon() {
        assert!(PROTOCOL_VERSION as usize > MAX_FRAME);
        let mut bytes = Vec::new();
        write_version(&mut bytes).expect("write");
        assert_eq!(bytes, PROTOCOL_VERSION.to_le_bytes());
        assert_eq!(read_version(&mut std::io::Cursor::new(bytes)).expect("read"), PROTOCOL_VERSION);
    }

    const GOLDEN_VERSION: u32 = 0x544d_000d;

    const GOLDEN_REQUESTS: [&str; REQUEST_VARIANTS] = [
        "3800000001010200000074320001010200000074360102000000743800020000000300000074313003000000c3a90901030000007431325000180000",
        "1300000002010200000074325000180002000000743300",
        "09000000030102000000743200",
        "080000000402000000743100",
        "0a00000005010200000074320000",
        "3100000006010200000074320001010001020000007438000101020000000300000074313203000000c3a909010300000074313400",
        "3800000007010200000074320001010001010200000074390103000000743131020000000300000074313203000000c3a90901030000007431340000",
        "12000000080102000000743200010200000074350000",
        "12000000090102000000743200010200000074350100",
        "090000000a0200000074310300",
        "0f0000001a0102000000743202000000743300",
        "090000000b0102000000743200",
        "090000000c0102000000743200",
        "0f0000000d0102000000743202000000743300",
        "0f0000000e0102000000743202000000743300",
        "0a0000000f010200000074320000",
        "0f000000100102000000743204010100010100",
        "110000001101020000007432000102000000743500",
        "110000001201020000007432030000000003ff0100",
        "090000002a0102000000743200",
        "1b00000013010200000074320001fbffffff00010100010300000074313100",
        "020000001400",
        "0f000000150102000000743200010100010100",
        "1300000016010200000074320200000074330400000100",
        "09000000350102000000743200",
        "09000000360102000000743200",
        "09000000370102000000743200",
        "1a00000038010200000074320200000002000000743303000000c3a90900",
        "110000001701020000007432020000007433010100",
        "18000000180102000000743200030000000004ff0102000000743600",
        "09000000190102000000743200",
        "0a0000001b010200000074320000",
        "090000001c0102000000743200",
        "130000001d010200000074320001030000000005ff0000",
        "100000001e01020000007432030000000003ff00",
        "100000001f010200000074320200000074330100",
        "1d0000002001020000007432010100010102000000743901030000007431310000",
        "230000002101020000007432000102000000743500010200000074380001030000007431310000",
        "0d00000022010200000074320001060000",
        "1900000023010200000074320001020000007435000102000000743800",
        "15000000240102000000743200010100010200000074380000",
        "2400000025010200000074320200000002000000743303000000c3a9090102000000743500010100",
        "09000000260102000000743200",
        "09000000330102000000743200",
        "0a00000027010200000074320000",
        "0b0000002801020000007432000100",
        "0e0000002901020000007432000101000100",
        "1c00000034010200000074320001020000007435020000007436010800000100",
        "0a0000002b010200000074320000",
        "180000002c0102000000743201010001010001010300000074313200",
        "190000002d010200000074320002000000743401020000007436010100",
        "120000002e0102000000743200010102000000743600",
        "0b0000002f01020000007432000100",
        "09000000300102000000743200",
        "0a00000031010200000074320000",
        "0f000000320102000000743202000000743300",
    ];

    #[test]
    fn wire_format_matches_goldens_for_the_current_version() {
        assert_eq!(
            PROTOCOL_VERSION, GOLDEN_VERSION,
            "bump PROTOCOL_VERSION and the goldens together"
        );
        let mut tags = Vec::new();
        for (variant, golden) in GOLDEN_REQUESTS.iter().enumerate() {
            let request = build(variant, &mut Canonical { calls: 0 });
            let frame = encode_request(&request, None).expect("encode");
            assert_eq!(hex(&frame), *golden, "request variant {variant}");
            tags.push(frame[4]);
        }
        tags.sort_unstable();
        let expected = (1..=REQUEST_VARIANTS as u8).collect::<Vec<_>>();
        assert_eq!(tags, expected, "every request tag needs exactly one golden");
        let next_tag = [1_u32.to_le_bytes().as_slice(), &[REQUEST_VARIANTS as u8 + 1]].concat();
        assert_eq!(
            decode_frame(&next_tag).expect_err("tag").to_string(),
            "unknown request tag",
            "a new request variant needs a golden"
        );
    }

    #[test]
    fn decoded_sizes_are_clamped_to_the_window_limits() {
        let mut bytes = vec![5, 0, 0, 0, 2];
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        let message = read_client_message(&mut bytes.as_slice()).expect("decode");
        assert_eq!(
            message,
            ClientMessage::Resize(Size::new(10_000, 1))
        );
    }

    #[test]
    fn wire_format_matches_goldens_for_messages() {
        let client = [
            (ClientMessage::Input(vec![0, 1, 255]), "0800000001030000000001ff"),
            (ClientMessage::Resize(Size::new(80, 24)), "0500000002500018 00"),
            (ClientMessage::Detach, "0100000003"),
        ];
        for (message, golden) in client {
            let mut bytes = Vec::new();
            write_client_message(&mut bytes, &message).expect("encode");
            assert_eq!(hex(&bytes), golden.replace(' ', ""), "{message:?}");
        }
        let server = [
            (
                ServerMessage::Response { ok: true, body: "ok".to_owned() },
                "08000000010102000000 6f6b",
            ),
            (ServerMessage::Render(vec![27, 91]), "0700000002020000001b5b"),
            (ServerMessage::Restore(vec![27, 91]), "0700000004020000001b5b"),
            (ServerMessage::Closed, "0100000003"),
            (
                ServerMessage::Output { status: 3, bytes: vec![27, 91] },
                "080000000503020000001b5b",
            ),
        ];
        for (message, golden) in server {
            let mut bytes = Vec::new();
            write_server_message(&mut bytes, &message).expect("encode");
            assert_eq!(hex(&bytes), golden.replace(' ', ""), "{message:?}");
        }
    }
}
