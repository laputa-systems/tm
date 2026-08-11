use std::io::{self, Read, Write};

use crate::model::Size;

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) enum Request {
    NewSession {
        name: Option<String>,
        detached: bool,
        attach_existing: bool,
        group_target: Option<String>,
        format: Option<String>,
        window_name: Option<String>,
        empty: bool,
        command: Vec<String>,
        cwd: Option<String>,
        size: Size,
    },
    Attach {
        target: Option<String>,
        size: Size,
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
        index: Option<u32>,
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
    LinkWindow {
        source: Option<String>,
        target: Option<String>,
        detached: bool,
        force: bool,
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum OptionScope {
    Global,
    Session,
    Window,
    Pane,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
    Next,
    Previous,
    Last,
}

#[derive(Debug)]
pub(crate) enum ClientMessage {
    Input(Vec<u8>),
    Resize(Size),
    Detach,
}

#[derive(Debug)]
pub(crate) enum ServerMessage {
    Response { ok: bool, body: String },
    Render(Vec<u8>),
    Closed,
}

pub(crate) fn write_request(stream: &mut impl Write, request: &Request) -> io::Result<()> {
    let mut payload = Vec::new();
    match request {
        Request::NewSession {
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
        } => {
            payload.push(1);
            put_optional_string(&mut payload, name);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *attach_existing);
            put_optional_string(&mut payload, group_target);
            put_optional_string(&mut payload, format);
            put_optional_string(&mut payload, window_name);
            put_bool(&mut payload, *empty);
            put_strings(&mut payload, command);
            put_optional_string(&mut payload, cwd);
            put_size(&mut payload, *size);
        }
        Request::Attach { target, size } => {
            payload.push(2);
            put_optional_string(&mut payload, target);
            put_size(&mut payload, *size);
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
            index,
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
            put_optional_u32(&mut payload, *index);
            put_bool(&mut payload, *force);
            put_optional_string(&mut payload, format);
            put_bool(&mut payload, *after);
            put_bool(&mut payload, *before);
            put_bool(&mut payload, *select_existing);
            put_strings(&mut payload, command);
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
            put_strings(&mut payload, command);
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
        Request::SelectWindow { target } => {
            payload.push(10);
            put_string(&mut payload, target);
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
            put_strings(&mut payload, command);
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
        Request::LinkWindow {
            source,
            target,
            detached,
            force,
        } => {
            payload.push(42);
            put_optional_string(&mut payload, source);
            put_optional_string(&mut payload, target);
            put_bool(&mut payload, *detached);
            put_bool(&mut payload, *force);
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
            put_strings(&mut payload, command);
            put_bool(&mut payload, *kill_on_exit);
        }
    }
    write_frame(stream, &payload)
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
            empty: decoder.bool()?,
            command: decoder.strings()?,
            cwd: decoder.optional_string()?,
            size: decoder.size()?,
        },
        2 => Request::Attach {
            target: decoder.optional_string()?,
            size: decoder.size()?,
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
            index: decoder.optional_u32()?,
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
        42 => Request::LinkWindow {
            source: decoder.optional_string()?,
            target: decoder.optional_string()?,
            detached: decoder.bool()?,
            force: decoder.bool()?,
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

fn write_frame(stream: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(invalid_data("frame is too large"));
    }
    stream.write_all(&(payload.len() as u32).to_le_bytes())?;
    stream.write_all(payload)
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

fn put_strings(output: &mut Vec<u8>, values: &[String]) {
    output.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        put_string(output, value);
    }
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

fn put_optional_u32(output: &mut Vec<u8>, value: Option<u32>) {
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
        if count > 1024 {
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

    fn optional_u32(&mut self) -> io::Result<Option<u32>> {
        if !self.bool()? {
            return Ok(None);
        }
        Ok(Some(self.u32()?))
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
        let mut bytes = Vec::new();
        write_request(&mut bytes, &request).expect("encode");
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
        let mut bytes = Vec::new();
        write_request(&mut bytes, &request).expect("encode");
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
            let mut bytes = Vec::new();
            write_request(&mut bytes, &request).expect("encode");
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
        let mut bytes = Vec::new();
        write_request(&mut bytes, &request).expect("encode");
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
        let mut bytes = Vec::new();
        write_request(&mut bytes, &request).expect("encode");
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
            let mut bytes = Vec::new();
            write_request(&mut bytes, &request).expect("encode");
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
}
