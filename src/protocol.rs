use std::io::{self, Read, Write};

use crate::model::Size;

const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_COMMAND_ARGUMENTS: usize = 1024;
pub(crate) const PROTOCOL_VERSION: u32 = 0x544d_0004;

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
        full: bool,
        detached: bool,
        empty: bool,
        zoom: bool,
        size: Option<String>,
        command: Vec<String>,
        cwd: Option<String>,
    },
    ListWindows {
        target: Option<String>,
        format: Option<String>,
    },
    ListPanes {
        target: Option<String>,
        format: Option<String>,
    },
    SelectWindow {
        target: String,
        last: bool,
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
    },
    KillServer,
    CopyMode {
        target: Option<String>,
        source: Option<String>,
        exit_on_scroll: bool,
        hide_position: bool,
        kill_on_exit: bool,
        page: bool,
        page_down: bool,
        reset: bool,
        mouse_start: bool,
        scroll_to_mouse: bool,
    },
    CopyModeCommand {
        target: Option<String>,
        action: String,
        repeat: u32,
    },
    ChooseTree {
        target: Option<String>,
        filter: Option<String>,
        format: Option<String>,
        sort: String,
        reverse: bool,
        hide_source: bool,
        kill_on_exit: bool,
    },
    ChooseBuffer {
        target: Option<String>,
        filter: Option<String>,
        format: Option<String>,
        sort: String,
        reverse: bool,
        kill_on_exit: bool,
    },
    ChooseClient {
        target: Option<String>,
        filter: Option<String>,
        format: Option<String>,
        kill_on_exit: bool,
    },
    DisplayPanes {
        target: Option<String>,
        source: Option<String>,
        no_zoom: bool,
        no_mode: bool,
        command: Vec<String>,
        kill_on_exit: bool,
    },
    DisplayMessage {
        target: Option<String>,
        format: String,
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
        raw: bool,
        bracketed: bool,
        separator: Option<Vec<u8>>,
        delete: bool,
    },
    LoadBuffer {
        name: Option<String>,
        data: Vec<u8>,
    },
    SaveBuffer {
        name: Option<String>,
        path: Option<String>,
        append: bool,
    },
    SetOption {
        target: Option<String>,
        scope: Option<OptionScope>,
        key: String,
        value: String,
        unset: bool,
    },
    ResizePane {
        target: Option<String>,
        direction: PaneDirection,
        amount: i32,
        absolute: Option<u16>,
        absolute_percent: bool,
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
    SetWindowOption {
        target: Option<String>,
        key: String,
        value: String,
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
        global: bool,
        window: bool,
        pane: bool,
        value: bool,
        all: bool,
        quiet: bool,
        key: Option<String>,
    },
    SetEnvironment {
        name: String,
        value: Option<String>,
        remove: bool,
    },
    ShowEnvironment {
        format: Option<String>,
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
        command: String,
        background: bool,
        target: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionScope {
    Global,
    Session,
    Window,
    Pane,
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

pub(crate) fn encode_request(request: &Request) -> io::Result<Vec<u8>> {
    framed(&request_payload(request)?)
}

fn request_payload(request: &Request) -> io::Result<Vec<u8>> {
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
            full,
            detached,
            empty,
            zoom,
            size,
            command,
            cwd,
        } => {
            payload.push(7);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *horizontal);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *full);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *empty);
            put_bool(&mut payload, *zoom);
            put_optional_string(&mut payload, size);
            put_strings(&mut payload, command)?;
            put_optional_string(&mut payload, cwd);
        }
        Request::ListWindows { target, format } => {
            payload.push(8);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, format);
        }
        Request::ListPanes { target, format } => {
            payload.push(9);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, format);
        }
        Request::SelectWindow { target, last } => {
            payload.push(10);
            put_string(&mut payload, target);
            put_bool(&mut payload, *last);
        }
        Request::SendPrefix { target } => {
            payload.push(42);
            put_optional_string(&mut payload, target);
        }
        Request::SelectLayout { target, layout } => {
            payload.push(57);
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
        } => {
            payload.push(19);
            put_optional_string(&mut payload, target);
            put_optional_i32(&mut payload, *start);
            put_optional_i32(&mut payload, *end);
            put_bool(&mut payload, *escape);
            put_bool(&mut payload, *join);
            put_bool(&mut payload, *preserve_trailing);
        }
        Request::KillServer => payload.push(20),
        Request::CopyMode {
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
        } => {
            payload.push(21);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, source);
            put_bool(&mut payload, *exit_on_scroll);
            put_bool(&mut payload, *hide_position);
            put_bool(&mut payload, *kill_on_exit);
            put_bool(&mut payload, *page);
            put_bool(&mut payload, *page_down);
            put_bool(&mut payload, *reset);
            put_bool(&mut payload, *mouse_start);
            put_bool(&mut payload, *scroll_to_mouse);
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
        Request::DisplayMessage { target, format } => {
            payload.push(23);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, format);
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
            raw,
            bracketed,
            separator,
            delete,
        } => {
            payload.push(29);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *raw);
            put_bool(&mut payload, *bracketed);
            match separator {
                Some(separator) => {
                    put_bool(&mut payload, true);
                    put_bytes(&mut payload, separator);
                }
                None => put_bool(&mut payload, false),
            }
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
            put_optional_string(&mut payload, path);
            put_bool(&mut payload, *append);
        }
        Request::SetOption {
            target,
            scope,
            key,
            value,
            unset,
        } => {
            payload.push(32);
            put_optional_string(&mut payload, target);
            match scope {
                Some(scope) => {
                    put_bool(&mut payload, true);
                    payload.push(option_scope_code(*scope));
                }
                None => put_bool(&mut payload, false),
            }
            put_string(&mut payload, key);
            put_string(&mut payload, value);
            put_bool(&mut payload, *unset);
        }
        Request::ResizePane {
            target,
            direction,
            amount,
            absolute,
            absolute_percent,
            zoom,
        } => {
            payload.push(33);
            put_optional_string(&mut payload, target);
            payload.push(direction_code(*direction));
            payload.extend_from_slice(&amount.to_le_bytes());
            put_optional_u16(&mut payload, *absolute);
            put_bool(&mut payload, *absolute_percent);
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
        } => {
            payload.push(36);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *horizontal);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *detached);
            put_optional_string(&mut payload, size);
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
        Request::SetWindowOption { target, key, value } => {
            payload.push(26);
            put_optional_string(&mut payload, target);
            put_string(&mut payload, key);
            put_string(&mut payload, value);
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
            global,
            window,
            pane,
            value,
            all,
            quiet,
            key,
        } => {
            payload.push(44);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *global);
            put_bool(&mut payload, *window);
            put_bool(&mut payload, *pane);
            put_bool(&mut payload, *value);
            put_bool(&mut payload, *all);
            put_bool(&mut payload, *quiet);
            put_optional_string(&mut payload, key);
        }
        Request::SetEnvironment {
            name,
            value,
            remove,
        } => {
            payload.push(45);
            put_string(&mut payload, name);
            put_optional_string(&mut payload, value);
            put_bool(&mut payload, *remove);
        }
        Request::ShowEnvironment { format, name } => {
            payload.push(46);
            put_optional_string(&mut payload, format);
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
        } => {
            payload.push(52);
            put_string(&mut payload, command);
            put_bool(&mut payload, *background);
            put_optional_string(&mut payload, target);
        }
        Request::ChooseTree {
            target,
            filter,
            format,
            sort,
            reverse,
            hide_source,
            kill_on_exit,
        } => {
            payload.push(53);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, filter);
            put_optional_string(&mut payload, format);
            put_string(&mut payload, sort);
            put_bool(&mut payload, *reverse);
            put_bool(&mut payload, *hide_source);
            put_bool(&mut payload, *kill_on_exit);
        }
        Request::ChooseBuffer {
            target,
            filter,
            format,
            sort,
            reverse,
            kill_on_exit,
        } => {
            payload.push(54);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, filter);
            put_optional_string(&mut payload, format);
            put_string(&mut payload, sort);
            put_bool(&mut payload, *reverse);
            put_bool(&mut payload, *kill_on_exit);
        }
        Request::ChooseClient {
            target,
            filter,
            format,
            kill_on_exit,
        } => {
            payload.push(55);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, filter);
            put_optional_string(&mut payload, format);
            put_bool(&mut payload, *kill_on_exit);
        }
        Request::DisplayPanes {
            target,
            source,
            no_zoom,
            no_mode,
            command,
            kill_on_exit,
        } => {
            payload.push(56);
            put_optional_string(&mut payload, target);
            put_optional_string(&mut payload, source);
            put_bool(&mut payload, *no_zoom);
            put_bool(&mut payload, *no_mode);
            put_strings(&mut payload, command)?;
            put_bool(&mut payload, *kill_on_exit);
        }
    }
    Ok(payload)
}

pub(crate) fn read_request(stream: &mut impl Read) -> io::Result<Request> {
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
            full: decoder.bool()?,
            detached: decoder.bool()?,
            empty: decoder.bool()?,
            zoom: decoder.bool()?,
            size: decoder.optional_string()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
        },
        8 => Request::ListWindows {
            target: decoder.optional_string()?,
            format: decoder.optional_string()?,
        },
        9 => Request::ListPanes {
            target: decoder.optional_string()?,
            format: decoder.optional_string()?,
        },
        10 => Request::SelectWindow {
            target: decoder.string()?,
            last: decoder.bool()?,
        },
        42 => Request::SendPrefix {
            target: decoder.optional_string()?,
        },
        57 => Request::SelectLayout {
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
        },
        20 => Request::KillServer,
        21 => Request::CopyMode {
            target: decoder.optional_string()?,
            source: decoder.optional_string()?,
            exit_on_scroll: decoder.bool()?,
            hide_position: decoder.bool()?,
            kill_on_exit: decoder.bool()?,
            page: decoder.bool()?,
            page_down: decoder.bool()?,
            reset: decoder.bool()?,
            mouse_start: decoder.bool()?,
            scroll_to_mouse: decoder.bool()?,
        },
        22 => Request::CopyModeCommand {
            target: decoder.optional_string()?,
            action: decoder.string()?,
            repeat: decoder.u32()?,
        },
        23 => Request::DisplayMessage {
            target: decoder.optional_string()?,
            format: decoder.string()?,
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
            raw: decoder.bool()?,
            bracketed: decoder.bool()?,
            separator: if decoder.bool()? {
                Some(decoder.bytes()?)
            } else {
                None
            },
            delete: decoder.bool()?,
        },
        30 => Request::LoadBuffer {
            name: decoder.optional_string()?,
            data: decoder.bytes()?,
        },
        31 => Request::SaveBuffer {
            name: decoder.optional_string()?,
            path: decoder.optional_string()?,
            append: decoder.bool()?,
        },
        32 => Request::SetOption {
            target: decoder.optional_string()?,
            scope: if decoder.bool()? {
                Some(decode_option_scope(decoder.byte()?)?)
            } else {
                None
            },
            key: decoder.string()?,
            value: decoder.string()?,
            unset: decoder.bool()?,
        },
        33 => Request::ResizePane {
            target: decoder.optional_string()?,
            direction: decode_direction(decoder.byte()?)?,
            amount: i32::from_le_bytes([
                decoder.byte()?,
                decoder.byte()?,
                decoder.byte()?,
                decoder.byte()?,
            ]),
            absolute: decoder.optional_u16()?,
            absolute_percent: decoder.bool()?,
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
            global: decoder.bool()?,
            window: decoder.bool()?,
            pane: decoder.bool()?,
            value: decoder.bool()?,
            all: decoder.bool()?,
            quiet: decoder.bool()?,
            key: decoder.optional_string()?,
        },
        45 => Request::SetEnvironment {
            name: decoder.string()?,
            value: decoder.optional_string()?,
            remove: decoder.bool()?,
        },
        46 => Request::ShowEnvironment {
            format: decoder.optional_string()?,
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
        52 => Request::RunShell {
            command: decoder.string()?,
            background: decoder.bool()?,
            target: decoder.optional_string()?,
        },
        53 => Request::ChooseTree {
            target: decoder.optional_string()?,
            filter: decoder.optional_string()?,
            format: decoder.optional_string()?,
            sort: decoder.string()?,
            reverse: decoder.bool()?,
            hide_source: decoder.bool()?,
            kill_on_exit: decoder.bool()?,
        },
        54 => Request::ChooseBuffer {
            target: decoder.optional_string()?,
            filter: decoder.optional_string()?,
            format: decoder.optional_string()?,
            sort: decoder.string()?,
            reverse: decoder.bool()?,
            kill_on_exit: decoder.bool()?,
        },
        55 => Request::ChooseClient {
            target: decoder.optional_string()?,
            filter: decoder.optional_string()?,
            format: decoder.optional_string()?,
            kill_on_exit: decoder.bool()?,
        },
        56 => Request::DisplayPanes {
            target: decoder.optional_string()?,
            source: decoder.optional_string()?,
            no_zoom: decoder.bool()?,
            no_mode: decoder.bool()?,
            command: decoder.strings()?,
            kill_on_exit: decoder.bool()?,
        },
        26 => Request::SetWindowOption {
            target: decoder.optional_string()?,
            key: decoder.string()?,
            value: decoder.string()?,
        },
        _ => return Err(invalid_data("unknown request tag")),
    };
    decoder.finish()?;
    Ok(request)
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

fn put_optional_i32(output: &mut Vec<u8>, value: Option<i32>) {
    match value {
        Some(value) => {
            put_bool(output, true);
            output.extend_from_slice(&value.to_le_bytes());
        }
        None => put_bool(output, false),
    }
}

fn put_optional_u16(output: &mut Vec<u8>, value: Option<u16>) {
    match value {
        Some(value) => {
            put_bool(output, true);
            output.extend_from_slice(&value.to_le_bytes());
        }
        None => put_bool(output, false),
    }
}

fn put_size(output: &mut Vec<u8>, size: Size) {
    output.extend_from_slice(&size.cols.to_le_bytes());
    output.extend_from_slice(&size.rows.to_le_bytes());
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

fn option_scope_code(scope: OptionScope) -> u8 {
    match scope {
        OptionScope::Global => 1,
        OptionScope::Session => 2,
        OptionScope::Window => 3,
        OptionScope::Pane => 4,
    }
}

fn decode_option_scope(code: u8) -> io::Result<OptionScope> {
    match code {
        1 => Ok(OptionScope::Global),
        2 => Ok(OptionScope::Session),
        3 => Ok(OptionScope::Window),
        4 => Ok(OptionScope::Pane),
        _ => Err(invalid_data("unknown option scope")),
    }
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

    fn optional_u16(&mut self) -> io::Result<Option<u16>> {
        if !self.bool()? {
            return Ok(None);
        }
        Ok(Some(self.u16()?))
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
            full: false,
            detached: false,
            empty: false,
            zoom: false,
            size: None,
            command: vec!["printf".to_owned(), "a\tb".to_owned()],
            cwd: Some("/tmp".to_owned()),
        };
        let bytes = encode_request(&request).expect("encode");
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
    fn choose_tree_request_round_trip_preserves_mode_contract() {
        let request = Request::ChooseTree {
            target: Some("work:1".to_owned()),
            filter: Some("#{==:#{session_name},work}".to_owned()),
            format: Some("#{session_name}:#{window_index}".to_owned()),
            sort: "name".to_owned(),
            reverse: true,
            hide_source: true,
            kill_on_exit: true,
        };
        let bytes = encode_request(&request).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
            Request::ChooseTree {
                target: Some(target),
                filter: Some(filter),
                format: Some(format),
                sort,
                reverse: true,
                hide_source: true,
                kill_on_exit: true,
            } if target == "work:1"
                && filter == "#{==:#{session_name},work}"
                && format == "#{session_name}:#{window_index}"
                && sort == "name"
        ));
    }

    #[test]
    fn buffer_and_client_chooser_requests_round_trip() {
        for request in [
            Request::ChooseBuffer {
                target: Some("work:0".to_owned()),
                filter: Some("#{==:#{buffer_name},log}".to_owned()),
                format: Some("#{buffer_name}".to_owned()),
                sort: "name".to_owned(),
                reverse: true,
                kill_on_exit: true,
            },
            Request::ChooseClient {
                target: None,
                filter: Some("#{==:#{client_session},work}".to_owned()),
                format: Some("#{client_session}".to_owned()),
                kill_on_exit: false,
            },
        ] {
            let bytes = encode_request(&request).expect("encode");
            let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
            let decoded = read_request_from_payload(&mut cursor).expect("decode");
            assert!(matches!(
                (request, decoded),
                (Request::ChooseBuffer { .. }, Request::ChooseBuffer { .. })
                    | (Request::ChooseClient { .. }, Request::ChooseClient { .. })
            ));
        }
    }

    #[test]
    fn paste_buffer_request_round_trip_preserves_bracketed_mode() {
        let request = Request::PasteBuffer {
            target: Some("work:0.1".to_owned()),
            name: Some("paste".to_owned()),
            raw: false,
            bracketed: true,
            separator: Some(b"|".to_vec()),
            delete: true,
        };
        let bytes = encode_request(&request).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
            Request::PasteBuffer {
                target: Some(target),
                name: Some(name),
                bracketed: true,
                separator: Some(separator),
                delete: true,
                ..
            } if target == "work:0.1" && name == "paste" && separator == b"|"
        ));
    }

    #[test]
    fn display_panes_request_round_trip_preserves_selection_contract() {
        let request = Request::DisplayPanes {
            target: Some("%3".to_owned()),
            source: Some("work:1".to_owned()),
            no_zoom: true,
            no_mode: false,
            command: vec![
                "set-option".to_owned(),
                "-g".to_owned(),
                "@picked".to_owned(),
                "%%".to_owned(),
            ],
            kill_on_exit: true,
        };
        let bytes = encode_request(&request).expect("encode");
        let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
        let decoded = read_request_from_payload(&mut cursor).expect("decode");
        assert!(matches!(
            decoded,
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
    fn copy_mode_request_round_trip_preserves_page_and_reset_options() {
        for request in [
            Request::CopyMode {
                target: Some("work:0".to_owned()),
                source: Some("work:1.2".to_owned()),
                exit_on_scroll: true,
                hide_position: true,
                kill_on_exit: false,
                page: false,
                page_down: true,
                reset: false,
                mouse_start: true,
                scroll_to_mouse: true,
            },
            Request::CopyMode {
                target: Some("work:0".to_owned()),
                source: None,
                exit_on_scroll: false,
                hide_position: false,
                kill_on_exit: false,
                page: false,
                page_down: false,
                reset: true,
                mouse_start: false,
                scroll_to_mouse: false,
            },
        ] {
            let bytes = encode_request(&request).expect("encode");
            let mut cursor = std::io::Cursor::new(bytes[4..].to_vec());
            let decoded = read_request_from_payload(&mut cursor).expect("decode");
            match (request, decoded) {
                (
                    Request::CopyMode {
                        source: Some(source),
                        mouse_start,
                        scroll_to_mouse,
                        ..
                    },
                    Request::CopyMode {
                        source: Some(decoded_source),
                        mouse_start: decoded_mouse_start,
                        scroll_to_mouse: decoded_scroll_to_mouse,
                        ..
                    },
                ) => {
                    assert_eq!(source, decoded_source);
                    assert!(mouse_start);
                    assert!(scroll_to_mouse);
                    assert_eq!(mouse_start, decoded_mouse_start);
                    assert_eq!(scroll_to_mouse, decoded_scroll_to_mouse);
                }
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
        read_request(&mut std::io::Cursor::new(framed))
    }

    const REQUEST_VARIANTS: usize = 57;

    trait Source {
        fn flag(&mut self) -> bool;
        fn opt_flag(&mut self) -> Option<bool>;
        fn text(&mut self) -> String;
        fn opt_text(&mut self) -> Option<String>;
        fn texts(&mut self) -> Vec<String>;
        fn blob(&mut self) -> Vec<u8>;
        fn opt_blob(&mut self) -> Option<Vec<u8>>;
        fn number(&mut self) -> u32;
        fn signed(&mut self) -> i32;
        fn opt_signed(&mut self) -> Option<i32>;
        fn opt_small(&mut self) -> Option<u16>;
        fn size(&mut self) -> Size;
        fn direction(&mut self) -> PaneDirection;
        fn opt_direction(&mut self) -> Option<PaneDirection>;
        fn opt_scope(&mut self) -> Option<OptionScope>;
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

    const SCOPES: [OptionScope; 4] = [
        OptionScope::Global,
        OptionScope::Session,
        OptionScope::Window,
        OptionScope::Pane,
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
        fn opt_blob(&mut self) -> Option<Vec<u8>> {
            self.toggle().then(|| self.blob())
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
        fn opt_small(&mut self) -> Option<u16> {
            self.calls += 1;
            self.toggle().then_some(0x0102 + self.calls as u16)
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
        fn opt_scope(&mut self) -> Option<OptionScope> {
            self.calls += 1;
            self.toggle().then(|| SCOPES[self.calls % SCOPES.len()])
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
        fn opt_blob(&mut self) -> Option<Vec<u8>> {
            self.flag().then(|| self.blob())
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
        fn opt_small(&mut self) -> Option<u16> {
            self.flag().then(|| self.next() as u16)
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
        fn opt_scope(&mut self) -> Option<OptionScope> {
            self.flag().then(|| SCOPES[self.below(SCOPES.len())])
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
                full: source.flag(),
                detached: source.flag(),
                empty: source.flag(),
                zoom: source.flag(),
                size: source.opt_text(),
                command: source.texts(),
                cwd: source.opt_text(),
        },
        7 => Request::ListWindows {
                target: source.opt_text(),
                format: source.opt_text(),
        },
        8 => Request::ListPanes {
                target: source.opt_text(),
                format: source.opt_text(),
        },
        9 => Request::SelectWindow {
                target: source.text(),
                last: source.flag(),
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
        },
        21 => Request::KillServer,
        22 => Request::CopyMode {
                target: source.opt_text(),
                source: source.opt_text(),
                exit_on_scroll: source.flag(),
                hide_position: source.flag(),
                kill_on_exit: source.flag(),
                page: source.flag(),
                page_down: source.flag(),
                reset: source.flag(),
                mouse_start: source.flag(),
                scroll_to_mouse: source.flag(),
        },
        23 => Request::CopyModeCommand {
                target: source.opt_text(),
                action: source.text(),
                repeat: source.number(),
        },
        24 => Request::ChooseTree {
                target: source.opt_text(),
                filter: source.opt_text(),
                format: source.opt_text(),
                sort: source.text(),
                reverse: source.flag(),
                hide_source: source.flag(),
                kill_on_exit: source.flag(),
        },
        25 => Request::ChooseBuffer {
                target: source.opt_text(),
                filter: source.opt_text(),
                format: source.opt_text(),
                sort: source.text(),
                reverse: source.flag(),
                kill_on_exit: source.flag(),
        },
        26 => Request::ChooseClient {
                target: source.opt_text(),
                filter: source.opt_text(),
                format: source.opt_text(),
                kill_on_exit: source.flag(),
        },
        27 => Request::DisplayPanes {
                target: source.opt_text(),
                source: source.opt_text(),
                no_zoom: source.flag(),
                no_mode: source.flag(),
                command: source.texts(),
                kill_on_exit: source.flag(),
        },
        28 => Request::DisplayMessage {
                target: source.opt_text(),
                format: source.text(),
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
                raw: source.flag(),
                bracketed: source.flag(),
                separator: source.opt_blob(),
                delete: source.flag(),
        },
        34 => Request::LoadBuffer {
                name: source.opt_text(),
                data: source.blob(),
        },
        35 => Request::SaveBuffer {
                name: source.opt_text(),
                path: source.opt_text(),
                append: source.flag(),
        },
        36 => Request::SetOption {
                target: source.opt_text(),
                scope: source.opt_scope(),
                key: source.text(),
                value: source.text(),
                unset: source.flag(),
        },
        37 => Request::ResizePane {
                target: source.opt_text(),
                direction: source.direction(),
                amount: source.signed(),
                absolute: source.opt_small(),
                absolute_percent: source.flag(),
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
        43 => Request::SetWindowOption {
                target: source.opt_text(),
                key: source.text(),
                value: source.text(),
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
                global: source.flag(),
                window: source.flag(),
                pane: source.flag(),
                value: source.flag(),
                all: source.flag(),
                quiet: source.flag(),
                key: source.opt_text(),
        },
        50 => Request::SetEnvironment {
                name: source.text(),
                value: source.opt_text(),
                remove: source.flag(),
        },
        51 => Request::ShowEnvironment {
                format: source.opt_text(),
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
        56 => Request::RefreshClient {
                target: source.opt_text(),
        },
        47 => Request::RunShell {
                command: source.text(),
                background: source.flag(),
                target: source.opt_text(),
        },
        _ => unreachable!("request variant index out of range"),
        }
    }

    fn decode_frame(frame: &[u8]) -> io::Result<Request> {
        read_request(&mut std::io::Cursor::new(frame.to_vec()))
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
                let frame = encode_request(&request).expect("encode");
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
        let frame = encode_request(&at_limit).expect("1024 arguments encode");
        assert_eq!(decode_frame(&frame).expect("decode"), at_limit);

        let error = encode_request(&new_window_with_commands(MAX_COMMAND_ARGUMENTS + 1))
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
            encode_request(&request).expect_err("oversized").to_string(),
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

    const GOLDEN_VERSION: u32 = 0x544d_0004;

    const GOLDEN_REQUESTS: [&str; REQUEST_VARIANTS] = [
        "3700000001010200000074320001010200000074360102000000743800020000000300000074313003000000c3a909010300000074313250001800",
        "12000000020102000000743250001800020000007433",
        "080000000301020000007432",
        "0700000004020000007431",
        "09000000050102000000743200",
        "3000000006010200000074320001010001020000007438000101020000000300000074313203000000c3a9090103000000743134",
        "29000000070102000000743200010100010100020000000300000074313003000000c3a9090103000000743132",
        "09000000080102000000743200",
        "09000000090102000000743200",
        "080000000a02000000743101",
        "0e0000003901020000007432020000007433",
        "080000000b01020000007432",
        "080000000c01020000007432",
        "0e0000000d01020000007432020000007433",
        "0e0000000e01020000007432020000007433",
        "090000000f0102000000743200",
        "0e0000001001020000007432040101000101",
        "1000000011010200000074320001020000007435",
        "100000001201020000007432030000000003ff01",
        "080000002a01020000007432",
        "1100000013010200000074320001fbffffff000101",
        "0100000014",
        "110000001501020000007432000101000101000101",
        "12000000160102000000743202000000743304000001",
        "1900000035010200000074320001020000007435020000007436010100",
        "18000000360102000000743200010200000074350200000074360101",
        "110000003701020000007432000102000000743500",
        "1d00000038010200000074320001010200000002000000743603000000c3a90901",
        "0e0000001701020000007432020000007433",
        "17000000180102000000743200030000000004ff01020000007436",
        "080000001901020000007432",
        "090000001b0102000000743200",
        "080000001c01020000007432",
        "0d0000001d010200000074320001010001",
        "0f0000001e01020000007432030000000003ff",
        "0a0000001f010200000074320001",
        "170000002001020000007432010102000000743502000000743601",
        "10000000210102000000743204fcffffff000101",
        "0c000000220102000000743200010600",
        "18000000230102000000743200010200000074350001020000007438",
        "1300000024010200000074320001010001020000007438",
        "2300000025010200000074320200000002000000743303000000c3a90901020000007435000101",
        "080000002601020000007432",
        "140000001a01020000007432020000007433020000007434",
        "09000000270102000000743200",
        "0a00000028010200000074320001",
        "0d00000029010200000074320001010001",
        "09000000340200000074310100",
        "090000002b0102000000743200",
        "0f0000002c0102000000743200010100010100",
        "0f0000002d0200000074310102000000743301",
        "090000002e0102000000743200",
        "0a0000002f010200000074320001",
        "080000003001020000007432",
        "09000000310102000000743200",
        "0e0000003201020000007432020000007433",
        "080000003301020000007432",
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
            let frame = encode_request(&request).expect("encode");
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
        ];
        for (message, golden) in server {
            let mut bytes = Vec::new();
            write_server_message(&mut bytes, &message).expect("encode");
            assert_eq!(hex(&bytes), golden.replace(' ', ""), "{message:?}");
        }
    }
}
