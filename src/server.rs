use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use vt100::{Color, Parser};

use crate::config;
#[cfg(test)]
use crate::config::ConfigLine;
use crate::copy_mode::{
    CopyAction, CopyLineNumberMode, CopyModeKeys, CopyModeState, CopyPromptKind, CopyScrollbarHit,
    DEFAULT_WORD_SEPARATORS, SelectionMode, display_column_to_char_index, display_prompt_input,
    history_rows, scrollbar_geometry_for, scrollbar_hit_for,
};
use crate::model::{Axis, CopySource, Pane, Rect, Session, Size, Window};
use crate::protocol::{
    ClientMessage, OptionScope, PaneDirection, Request, ServerMessage, read_client_message,
    read_request, write_server_message,
};
use crate::pty::Pty;
use crate::terminal;

type SharedState = Arc<Mutex<ServerState>>;
type CommandResult = Result<String, String>;

// Attach clients sleep until a state mutation can affect their screen. The
// condition variable is process-wide because each daemon owns one state
// mutex; spurious wakeups are harmless and avoid a timer-driven render loop.
static RENDER_WAKE: Condvar = Condvar::new();

pub(crate) fn socket_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_owned();
    }
    if let Some(path) = std::env::var_os("TM_SOCKET") {
        return PathBuf::from(path);
    }
    let uid = unsafe { libc::geteuid() };
    std::env::temp_dir().join(format!("tm-{uid}.sock"))
}

fn binding_table(bindings: &[config::CompiledBinding]) -> HashMap<Vec<u8>, ConfigBinding> {
    bindings
        .iter()
        .map(|binding| {
            let key = config::key_bytes(binding.key)
                .expect("compiled binding must use a supported terminal key");
            let commands = binding
                .commands
                .iter()
                .map(|command| command.iter().map(|value| (*value).to_owned()).collect())
                .collect();
            (
                key,
                ConfigBinding {
                    _repeat: binding.repeat,
                    commands,
                },
            )
        })
        .collect()
}

pub(crate) fn run_daemon(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    let listener = UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    let mut initial_state = ServerState::new();
    // A private `TM_SOCKET` remains an isolated vanilla server for tests and
    // tooling. The normal daemon overlays the compiled interactive profile.
    if std::env::var_os("TM_SOCKET").is_none() {
        initial_state.apply_compiled_interactive_config();
    }
    let state = Arc::new(Mutex::new(initial_state));

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                thread::Builder::new()
                    .name("tm-client".to_owned())
                    .spawn(move || handle_connection(stream, state))
                    .map_err(io::Error::other)?;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if state.lock().map_err(|_| poisoned())?.shutdown {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }

    let _ = fs::remove_file(path);
    Ok(())
}

fn handle_connection(mut stream: UnixStream, shared: SharedState) {
    // `run_daemon` keeps the listener nonblocking for its accept loop. On
    // platforms where accepted Unix sockets inherit that flag, an attached
    // client's input reader would observe EAGAIN before the first key and
    // tear down an otherwise healthy session. Requests and the long-lived
    // attach stream are intentionally blocking at this boundary.
    let _ = stream.set_nonblocking(false);
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(_) => return,
    };
    if let Request::Attach { target, size } = request {
        attach_client(stream, shared, target, size);
        return;
    }

    let result = match shared.lock() {
        Ok(mut state) => {
            let result = execute_request(&mut state, &shared, request);
            state.mark_render_dirty();
            result
        }
        Err(_) => Err("server state lock is poisoned".to_owned()),
    };
    let (ok, body) = match result {
        Ok(body) => (true, body),
        Err(error) => (false, error),
    };
    let _ = write_server_message(&mut stream, &ServerMessage::Response { ok, body });
}

fn attach_client(mut stream: UnixStream, shared: SharedState, target: Option<String>, size: Size) {
    let (_session_id, client_id) = match shared.lock() {
        Ok(mut state) => match state.register_client(target.as_deref(), size) {
            Ok(client) => {
                state.mark_render_dirty();
                client
            }
            Err(error) => {
                let _ = write_server_message(
                    &mut stream,
                    &ServerMessage::Response {
                        ok: false,
                        body: error,
                    },
                );
                return;
            }
        },
        Err(_) => {
            let _ = write_server_message(
                &mut stream,
                &ServerMessage::Response {
                    ok: false,
                    body: "server state lock is poisoned".to_owned(),
                },
            );
            return;
        }
    };

    if write_server_message(
        &mut stream,
        &ServerMessage::Response {
            ok: true,
            body: String::new(),
        },
    )
    .is_err()
    {
        return;
    }

    let mut input_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let input_alive = Arc::clone(&alive);
    let input_state = Arc::clone(&shared);
    thread::Builder::new()
        .name("tm-attach-input".to_owned())
        .spawn(move || {
            while input_alive.load(std::sync::atomic::Ordering::Acquire) {
                let message = match read_client_message(&mut input_stream) {
                    Ok(message) => message,
                    Err(_) => break,
                };
                if let Ok(mut state) = input_state.lock() {
                    match message {
                        ClientMessage::Input(bytes) => {
                            state.handle_client_input(client_id, &bytes, &input_state);
                            state.mark_render_dirty();
                        }
                        ClientMessage::Resize(size) => {
                            if let Some(session_id) = state
                                .clients
                                .get(&client_id)
                                .map(|client| client.session_id)
                            {
                                if let Some(client) = state.clients.get_mut(&client_id) {
                                    client.size = size;
                                }
                                state.resize_session(session_id, size);
                            }
                            state.mark_render_dirty();
                        }
                        ClientMessage::Detach => {
                            state.clients.remove(&client_id);
                            state.mark_render_dirty();
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
            input_alive.store(false, std::sync::atomic::Ordering::Release);
            RENDER_WAKE.notify_all();
        })
        .ok();

    let mut previous = Vec::new();
    let mut terminal = vt100::Parser::new(size.rows.max(1), size.cols.max(1), 10_000);
    let mut previous_screen = None;
    let mut last_size = None;
    let mut observed_revision = None;
    let mut last_render_at = Instant::now()
        .checked_sub(Duration::from_millis(16))
        .unwrap_or_else(Instant::now);
    while alive.load(std::sync::atomic::Ordering::Acquire) {
        let next_render = match shared.lock() {
            Ok(mut state) => loop {
                let Some(client) = state.clients.get(&client_id) else {
                    break None;
                };
                let session_id = client.session_id;
                let render_size = client.size.bounded();
                let revision = state.render_revision;
                if observed_revision == Some(revision) && last_size == Some(render_size) {
                    state = match RENDER_WAKE.wait(state) {
                        Ok(state) => state,
                        Err(_) => break None,
                    };
                    continue;
                }
                let render_wait =
                    Duration::from_millis(16).saturating_sub(last_render_at.elapsed());
                if !render_wait.is_zero() {
                    state = match RENDER_WAKE.wait_timeout(state, render_wait) {
                        Ok((state, _)) => state,
                        Err(_) => break None,
                    };
                    continue;
                }
                let clear_screen = last_size != Some(render_size);
                let Some(full_render) =
                    state.render_session_with_clear(session_id, Some(client_id), clear_screen)
                else {
                    break None;
                };
                break Some((full_render, render_size, revision));
            },
            Err(_) => None,
        };
        let Some((full_render, render_size, render_revision)) = next_render else {
            break;
        };
        // Keep the revision observed before rendering. Rendering consumes
        // one-shot state such as messages and clipboard notifications; those
        // mutations intentionally make the next loop produce a follow-up
        // frame.
        observed_revision = Some(render_revision);
        last_render_at = Instant::now();
        if last_size != Some(render_size) {
            terminal = vt100::Parser::new(render_size.rows.max(1), render_size.cols.max(1), 10_000);
            previous_screen = None;
            previous.clear();
            last_size = Some(render_size);
        }
        let render = if previous_screen.is_none() {
            terminal.process(&full_render);
            previous_screen = Some(terminal.screen().clone());
            full_render
        } else {
            let before = terminal.screen().clone();
            terminal.process(&full_render);
            let after = terminal.screen().clone();
            // Screen diffs cannot carry OSC/DCS side effects (for example an
            // OSC 52 clipboard update), so preserve the complete non-clearing
            // frame whenever one is present.
            let delta = if full_render
                .windows(2)
                .any(|window| window == b"\x1b]" || window == b"\x1bP")
            {
                Err(())
            } else {
                render_screen_delta(&before, &after)
            };
            previous_screen = Some(after);
            match delta {
                Ok(Some(delta)) => delta,
                Ok(None) => Vec::new(),
                Err(()) => full_render,
            }
        };
        if !render.is_empty() && render != previous {
            if write_server_message(&mut stream, &ServerMessage::Render(render.clone())).is_err() {
                break;
            }
            let _ = stream.flush();
            previous = render;
        }
    }
    alive.store(false, std::sync::atomic::Ordering::Release);
    if let Ok(mut state) = shared.lock() {
        state.clients.remove(&client_id);
        state.mark_render_dirty();
    }
    let _ = write_server_message(&mut stream, &ServerMessage::Closed);
}

struct ServerState {
    // Monotonic generation for screen-affecting state. Attach clients wait
    // for this to change instead of rebuilding an unchanged frame on a timer.
    render_revision: u64,
    sessions: Vec<Session>,
    next_session_id: u64,
    next_window_id: u64,
    next_pane_id: u64,
    next_buffer_id: u64,
    buffers: Vec<Buffer>,
    buffer_limit: usize,
    global_options: HashMap<String, String>,
    environment: HashMap<String, String>,
    pane_pipes: HashMap<u64, PanePipe>,
    clients: HashMap<u64, AttachedClient>,
    next_client_id: u64,
    remain_on_exit: bool,
    marked_pane: Option<u64>,
    mode_keys: CopyModeKeys,
    word_separators: String,
    synchronize_panes: bool,
    shutdown: bool,
    prefix: Vec<u8>,
    bindings: HashMap<Vec<u8>, ConfigBinding>,
    history_limit: usize,
    prompt_history: HashMap<String, Vec<String>>,
    last_message: Option<String>,
    clipboard_pending: Option<Vec<u8>>,
    table_bindings: HashMap<(String, Vec<u8>), ConfigBinding>,
    mouse_bindings: HashMap<(String, String), ConfigBinding>,
    mouse_context: Option<MouseContext>,
}

/// A deterministic in-process render fixture for performance benchmarks.
///
/// It deliberately uses empty panes, so benchmark setup never measures shell
/// startup or PTY scheduling. The returned frame is the same byte stream sent
/// to an attached client after pane state has been parsed.
#[doc(hidden)]
#[allow(dead_code)]
pub struct RenderBenchmark {
    state: ServerState,
    session_id: u64,
    client_id: u64,
    terminal: vt100::Parser,
    previous_screen: Option<vt100::Screen>,
}

#[allow(dead_code)]
impl RenderBenchmark {
    #[doc(hidden)]
    pub fn new(cols: u16, rows: u16, pane_count: usize) -> Self {
        assert!(pane_count > 0, "render benchmark needs at least one pane");
        let size = Size::new(cols, rows).bounded();
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = ServerState::new();
        state.apply_compiled_interactive_config();
        state
            .create_session(
                &shared,
                Some("render-benchmark"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                size,
            )
            .expect("create render benchmark session");
        for _ in 1..pane_count {
            state
                .split_window(
                    &shared,
                    Some("render-benchmark:0"),
                    true,
                    false,
                    false,
                    true,
                    false,
                    true,
                    None,
                    &[],
                    None,
                )
                .expect("split render benchmark pane");
        }
        let session_id = state.sessions[0].id;
        let (_, client_id) = state
            .register_client(Some("render-benchmark"), size)
            .expect("register render benchmark client");
        for pane in &mut state.sessions[0].windows[0].panes {
            pane.parser.process(
                b"render benchmark 0123456789 abcdefghijklmnopqrstuvwxyz\r\nsecond line\r\n",
            );
        }
        Self {
            state,
            session_id,
            client_id,
            terminal: vt100::Parser::new(size.rows, size.cols, 10_000),
            previous_screen: None,
        }
    }

    #[doc(hidden)]
    pub fn render_frame(&mut self) -> Vec<u8> {
        self.state
            .render_session(self.session_id, Some(self.client_id))
            .expect("render benchmark session")
    }

    #[doc(hidden)]
    pub fn render_delta_frame(&mut self) -> Vec<u8> {
        let full = self
            .state
            .render_session_with_clear(
                self.session_id,
                Some(self.client_id),
                self.previous_screen.is_none(),
            )
            .expect("render benchmark session");
        if self.previous_screen.is_none() {
            self.terminal.process(&full);
            self.previous_screen = Some(self.terminal.screen().clone());
            return full;
        }
        let before = self.terminal.screen().clone();
        self.terminal.process(&full);
        let after = self.terminal.screen().clone();
        self.previous_screen = Some(after.clone());
        match render_screen_delta(&before, &after) {
            Ok(Some(delta)) => delta,
            Ok(None) => Vec::new(),
            Err(()) => full,
        }
    }
}

#[derive(Clone, Debug)]
struct ConfigBinding {
    _repeat: bool,
    commands: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
struct MouseContext {
    x: usize,
    y: usize,
    pane_id: u64,
    word: String,
    line: String,
    hyperlink: String,
    button: u16,
}

struct Buffer {
    name: String,
    data: Vec<u8>,
    automatic: bool,
    created: i64,
}

struct PanePipe {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Child,
}

struct AttachedClient {
    session_id: u64,
    size: Size,
    prefix_pending: bool,
    key_buffer: Vec<u8>,
    input_buffer: Vec<u8>,
    root_key_buffer: Vec<u8>,
    copy_key_buffer: Vec<u8>,
    repeat_key: Option<Vec<u8>>,
    repeat_binding: Option<ConfigBinding>,
    mouse_buffer: Vec<u8>,
    mouse_drag_button: Option<u16>,
    mouse_slider_offset: Option<usize>,
    mouse_resize: Option<MouseResize>,
    last_mouse_click: Option<MouseClickState>,
    prompt: Option<AttachedPrompt>,
    tree_mode: Option<TreeMode>,
    buffer_mode: Option<BufferMode>,
    client_mode: Option<ClientMode>,
    panes_mode: Option<PaneDisplayMode>,
}

#[derive(Clone, Debug)]
struct MouseResize {
    button: u16,
    axis: Axis,
    path: Vec<bool>,
    start_coordinate: u16,
    start_size: u16,
}

#[derive(Clone, Debug)]
struct PaneDisplayMode {
    entries: Vec<PaneDisplayEntry>,
    target_pane: u64,
    previous_zoomed: bool,
    command: Option<Vec<String>>,
    kill_on_exit: bool,
}

#[derive(Clone, Debug)]
struct PaneDisplayEntry {
    pane_id: u64,
    index: u32,
    text: String,
}

#[derive(Clone, Debug)]
struct TreeMode {
    entries: Vec<TreeEntry>,
    cursor: usize,
    filter: Option<String>,
    filter_input: Option<Vec<u8>>,
    format: String,
    sort: TreeSort,
    reverse: bool,
    hide_source: bool,
    source_pane: Option<u64>,
    collapsed: HashSet<TreeKey>,
    no_matches: bool,
    confirmation: Option<TreeEntry>,
    confirmation_label: Option<String>,
    kill_on_exit: bool,
}

#[derive(Clone, Debug)]
struct TreeEntry {
    session_id: u64,
    window_id: Option<u64>,
    pane_id: Option<u64>,
    key: TreeKey,
    text: String,
}

#[derive(Clone, Debug)]
struct BufferMode {
    entries: Vec<BufferEntry>,
    cursor: usize,
    filter: Option<String>,
    filter_input: Option<Vec<u8>>,
    format: String,
    sort: TreeSort,
    reverse: bool,
    no_matches: bool,
    source_pane: u64,
    tagged: HashSet<String>,
    kill_on_exit: bool,
}

#[derive(Clone, Debug)]
struct BufferEntry {
    name: String,
    text: String,
}

#[derive(Clone, Debug)]
struct ClientMode {
    entries: Vec<ClientEntry>,
    cursor: usize,
    filter: Option<String>,
    filter_input: Option<Vec<u8>>,
    format: String,
    no_matches: bool,
    source_pane: u64,
    kill_on_exit: bool,
}

#[derive(Clone, Debug)]
struct ClientEntry {
    client_id: u64,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TreeKey {
    Session(u64),
    Window(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeSort {
    Index,
    Name,
}

#[derive(Clone, Copy, Debug)]
struct MouseClickState {
    button: u16,
    col: u16,
    row: u16,
    count: u8,
    at: Instant,
}

struct AttachedPrompt {
    command: String,
    prompt_type: String,
    label: String,
    labels: Vec<String>,
    initial_inputs: Vec<Vec<u8>>,
    current_prompt: usize,
    accepted_inputs: Vec<String>,
    input: Vec<u8>,
    cursor: usize,
    history_index: usize,
    quoted: bool,
    yank_buffer: Vec<u8>,
    mode: AttachedPromptMode,
    backspace_exit: bool,
    incremental: bool,
    pane: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AttachedPromptMode {
    #[default]
    Line,
    Single,
    Numeric,
    Key,
}

impl AttachedPrompt {
    fn move_left(&mut self) {
        self.cursor = previous_utf8_boundary(&self.input, self.cursor);
    }

    fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor += std::str::from_utf8(&self.input[self.cursor..])
                .ok()
                .and_then(|value| value.chars().next())
                .map_or(1, char::len_utf8);
        }
    }

    fn move_start(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.input.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = previous_utf8_boundary(&self.input, self.cursor);
        self.input.drain(start..self.cursor);
        self.cursor = start;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let end = self.cursor
            + std::str::from_utf8(&self.input[self.cursor..])
                .ok()
                .and_then(|value| value.chars().next())
                .map_or(1, char::len_utf8);
        self.input.drain(self.cursor..end);
    }

    fn kill_to_start(&mut self) -> Vec<u8> {
        let killed = self.input[..self.cursor].to_vec();
        self.input.drain(..self.cursor);
        self.cursor = 0;
        killed
    }

    fn kill_to_end(&mut self) -> Vec<u8> {
        let killed = self.input[self.cursor..].to_vec();
        self.input.truncate(self.cursor);
        killed
    }

    fn kill_word(&mut self) -> Vec<u8> {
        let original_cursor = self.cursor;
        let mut start = self.cursor;
        while start > 0 {
            let previous = previous_utf8_boundary(&self.input, start);
            let character = std::str::from_utf8(&self.input[previous..start])
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or(' ');
            if !character.is_whitespace() {
                break;
            }
            start = previous;
        }
        while start > 0 {
            let previous = previous_utf8_boundary(&self.input, start);
            let character = std::str::from_utf8(&self.input[previous..start])
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or(' ');
            if character.is_whitespace() {
                break;
            }
            start = previous;
        }
        let killed = self.input[start..original_cursor].to_vec();
        self.input.drain(start..self.cursor);
        self.cursor = start;
        killed
    }

    fn yank(&mut self) {
        if self.yank_buffer.is_empty() {
            return;
        }
        self.input
            .splice(self.cursor..self.cursor, self.yank_buffer.iter().copied());
        self.cursor += self.yank_buffer.len();
    }

    fn current_input(&self) -> String {
        String::from_utf8_lossy(&self.input).into_owned()
    }

    fn cursor_display(&self) -> String {
        let input = self.input.get(..self.cursor).unwrap_or(&self.input);
        format!(
            "{}{}",
            self.label,
            display_prompt_input(input)
        )
    }

    fn all_inputs(&self) -> Vec<String> {
        let mut inputs = self.accepted_inputs.clone();
        inputs.push(self.current_input());
        inputs
    }

    fn expanded_command(&self) -> String {
        let inputs = self.all_inputs();
        if self.command.is_empty() {
            return inputs.last().cloned().unwrap_or_default();
        }
        let mut command = self.command.clone();
        for (index, input) in inputs.iter().enumerate() {
            command = command.replace(&format!("%{}", index + 1), input);
        }
        if inputs.len() == 1 {
            command = command.replace("%%", &inputs[0]);
        }
        command
    }

    fn advance_prompt(&mut self) -> bool {
        if self.current_prompt + 1 >= self.labels.len() {
            return false;
        }
        self.accepted_inputs.push(self.current_input());
        self.current_prompt += 1;
        self.label = self.labels[self.current_prompt].clone();
        self.input = self
            .initial_inputs
            .get(self.current_prompt)
            .cloned()
            .unwrap_or_default();
        self.cursor = self.input.len();
        self.yank_buffer.clear();
        true
    }

    fn insert(&mut self, byte: u8) {
        self.input.insert(self.cursor, byte);
        self.cursor += 1;
    }

    fn complete_command(&mut self) {
        const COMMANDS: &[&str] = &[
            "attach-session",
            "bind-key",
            "break-pane",
            "capture-pane",
            "choose-buffer",
            "choose-client",
            "choose-tree",
            "clear-history",
            "command-prompt",
            "copy-mode",
            "delete-buffer",
            "detach-client",
            "display-message",
            "display-panes",
            "find-window",
            "join-pane",
            "kill-pane",
            "kill-session",
            "kill-window",
            "list-buffers",
            "list-clients",
            "list-panes",
            "list-sessions",
            "list-windows",
            "load-buffer",
            "move-pane",
            "move-window",
            "new-session",
            "new-window",
            "next-window",
            "paste-buffer",
            "pipe-pane",
            "previous-window",
            "refresh-client",
            "rename-session",
            "rename-window",
            "resize-pane",
            "respawn-pane",
            "respawn-window",
            "rotate-window",
            "run-shell",
            "save-buffer",
            "select-layout",
            "select-pane",
            "select-window",
            "send-keys",
            "send-prefix",
            "set-buffer",
            "set-hook",
            "set-option",
            "set-window-option",
            "show-buffer",
            "show-environment",
            "show-messages",
            "show-options",
            "show-window-options",
            "split-window",
            "swap-pane",
            "swap-window",
            "switch-client",
            "unbind-key",
            "unlink-window",
        ];
        let input = String::from_utf8_lossy(&self.input).into_owned();
        let matches = COMMANDS
            .iter()
            .filter(|command| command.starts_with(&input))
            .copied()
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            self.input = matches[0].as_bytes().to_vec();
            self.cursor = self.input.len();
        }
    }

    fn history_up(&mut self, history: &[String]) {
        if history.is_empty() {
            return;
        }
        self.history_index = self.history_index.saturating_sub(1);
        self.input = history[self.history_index].as_bytes().to_vec();
        self.cursor = self.input.len();
    }

    fn history_down(&mut self, history: &[String]) {
        if self.history_index + 1 < history.len() {
            self.history_index += 1;
            self.input = history[self.history_index].as_bytes().to_vec();
        } else {
            self.history_index = history.len();
            self.input.clear();
        }
        self.cursor = self.input.len();
    }
}

impl Drop for PanePipe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ServerState {
    fn new() -> Self {
        Self {
            render_revision: 0,
            sessions: Vec::new(),
            next_session_id: 0,
            next_window_id: 0,
            next_pane_id: 0,
            next_buffer_id: 0,
            buffers: Vec::new(),
            buffer_limit: 50,
            global_options: HashMap::new(),
            environment: HashMap::new(),
            pane_pipes: HashMap::new(),
            clients: HashMap::new(),
            next_client_id: 0,
            remain_on_exit: false,
            marked_pane: None,
            mode_keys: CopyModeKeys::Emacs,
            word_separators: DEFAULT_WORD_SEPARATORS.to_owned(),
            synchronize_panes: false,
            shutdown: false,
            prefix: vec![2],
            bindings: binding_table(config::VANILLA_BINDINGS),
            history_limit: 10_000,
            prompt_history: HashMap::new(),
            last_message: None,
            clipboard_pending: None,
            table_bindings: HashMap::new(),
            mouse_bindings: HashMap::new(),
            mouse_context: None,
        }
    }

    fn mark_render_dirty(&mut self) {
        self.render_revision = self.render_revision.wrapping_add(1);
        RENDER_WAKE.notify_all();
    }

    fn apply_compiled_interactive_config(&mut self) {
        // `config::COMPILED_OPTIONS` is the sole source of interactive
        // startup settings. Startup must never inspect tmux.conf or TM_CONFIG.
        for &(key, value) in config::COMPILED_OPTIONS {
            self.set_global_option(key, value, false)
                .expect("compiled option must be valid");
        }
        self.bindings
            .extend(binding_table(config::COMPILED_BINDINGS));
    }

    fn create_session(
        &mut self,
        shared: &SharedState,
        name: Option<&str>,
        attach_existing: bool,
        group_target: Option<&str>,
        format: Option<&str>,
        window_name: Option<&str>,
        empty: bool,
        command: &[String],
        cwd: Option<&str>,
        size: Size,
    ) -> CommandResult {
        let session_name = match name.filter(|name| !name.is_empty()) {
            Some(name) => render_format_with_options(
                name,
                &[("pid", std::process::id().to_string())],
                &self.global_options,
            ),
            None => self.next_session_name(),
        };
        if attach_existing
            && let Some(session) = self
                .sessions
                .iter()
                .find(|session| session.name == session_name)
        {
            return Ok(format_session_result(session, format));
        }
        if self
            .sessions
            .iter()
            .any(|session| session.name == session_name)
        {
            return Err(format!("duplicate session: {session_name}"));
        }
        if let Some(group_target) = group_target {
            if !command.is_empty() {
                return Err("grouped session cannot specify a command".to_owned());
            }
            let source_index = self.resolve_session_index(Some(group_target))?;
            let source = &self.sessions[source_index];
            let windows = source
                .windows
                .iter()
                .map(Window::linked_clone)
                .collect::<Vec<_>>();
            let session_id = self.next_session_id;
            self.next_session_id += 1;
            self.sessions.push(Session {
                id: session_id,
                name: session_name.clone(),
                size: source.size,
                windows,
                active_window: source.active_window,
                last_window: source.last_window,
                base_index: source.base_index,
                renumber_windows: source.renumber_windows,
                next_window_index: source.next_window_index,
                cwd: source.cwd.clone(),
                options: source.options.clone(),
            });
            self.reflow_session(session_id);
            let session = self
                .sessions
                .iter()
                .find(|session| session.name == session_name)
                .ok_or_else(|| "new grouped session no longer exists".to_owned())?;
            return Ok(format_session_result(session, format));
        }
        let size = size.bounded();
        let base_index = self.global_base_index();
        let pane_id = self.next_pane_id();
        let pane = self.new_pane(shared, pane_id, 0, size, command, cwd, empty)?;
        let window_id = self.next_window_id;
        self.next_window_id += 1;
        let mut window = Window::new(
            window_id,
            base_index,
            window_name
                .map(|name| {
                    render_format_with_options(
                        name,
                        &[
                            ("pid", std::process::id().to_string()),
                            ("session_name", session_name.clone()),
                        ],
                        &self.global_options,
                    )
                })
                .unwrap_or_else(|| "0".to_owned()),
            size,
            pane,
        );
        window.mode_keys = self.mode_keys;
        window.word_separators = self.word_separators.clone();
        window.synchronize_panes = self.synchronize_panes;
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        self.sessions.push(Session {
            id: session_id,
            name: session_name.clone(),
            size,
            windows: vec![window],
            active_window: base_index,
            last_window: None,
            base_index,
            renumber_windows: self
                .global_options
                .get("renumber-windows")
                .is_some_and(|value| parse_on_off(value).unwrap_or(false)),
            next_window_index: base_index.saturating_add(1),
            cwd: cwd.map(str::to_owned),
            options: HashMap::new(),
        });
        self.reflow_session(session_id);
        let session = self
            .sessions
            .iter()
            .find(|session| session.name == session_name)
            .ok_or_else(|| "new session no longer exists".to_owned())?;
        Ok(format_session_result(session, format))
    }

    fn global_base_index(&self) -> u32 {
        self.global_options
            .get("base-index")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0)
    }

    fn new_window(
        &mut self,
        shared: &SharedState,
        target: Option<&str>,
        name: Option<&str>,
        detached: bool,
        requested_index: Option<u32>,
        force: bool,
        format: Option<&str>,
        after: bool,
        before: bool,
        select_existing: bool,
        empty: bool,
        command: &[String],
        cwd: Option<&str>,
    ) -> CommandResult {
        let session_index = self.resolve_session_index(target)?;
        if select_existing
            && let Some(name) = name
            && let Some(window) = self.sessions[session_index]
                .windows
                .iter()
                .find(|window| window.name == name)
        {
            let index = window.index;
            if !detached {
                self.sessions[session_index].select_window(index);
            }
            return Ok(String::new());
        }
        let requested_index = requested_index.or_else(|| {
            target
                .and_then(|target| target.rsplit_once(':').map(|(_, window)| window))
                .and_then(|window| window.parse::<u32>().ok())
        });
        let insertion_index = if after || before {
            let (_, target_window) = self.resolve_window_target(target)?;
            let target_number = self.sessions[session_index].windows[target_window].index;
            Some(if after {
                target_number.saturating_add(1)
            } else {
                target_number
            })
        } else {
            requested_index
        };
        if after || before {
            let index = insertion_index.expect("window insertion always has an index");
            let active_window = self.sessions[session_index].active_window;
            for window in &mut self.sessions[session_index].windows {
                if window.index >= index {
                    window.index = window.index.saturating_add(1);
                }
            }
            if active_window >= index {
                self.sessions[session_index].active_window = active_window.saturating_add(1);
            }
            if let Some(last_window) = self.sessions[session_index].last_window
                && last_window >= index
            {
                self.sessions[session_index].last_window = Some(last_window.saturating_add(1));
            }
        }
        let (session_id, size, window_index, session_cwd) = {
            let session = &mut self.sessions[session_index];
            let index = insertion_index.unwrap_or_else(|| {
                (session.base_index..=session.next_window_index)
                    .find(|index| session.windows.iter().all(|window| window.index != *index))
                    .unwrap_or(session.next_window_index)
            });
            (session.id, session.size, index, session.cwd.clone())
        };
        if let Some(existing) = self.sessions[session_index]
            .windows
            .iter()
            .position(|window| window.index == window_index)
        {
            if !force {
                return Err(format!("create window failed: index {window_index} in use"));
            }
            let old = self.sessions[session_index].windows.remove(existing);
            for pane in old.panes {
                pane.pty.kill();
            }
        }
        let pane_id = self.next_pane_id();
        let pane = self.new_pane(
            shared,
            pane_id,
            0,
            size,
            command,
            cwd.or(session_cwd.as_deref()),
            empty,
        )?;
        let window_name = name
            .map(|name| {
                render_format_with_options(
                    name,
                    &[
                        ("pid", std::process::id().to_string()),
                        ("session_name", self.sessions[session_index].name.clone()),
                    ],
                    &self.global_options,
                )
            })
            .unwrap_or_else(|| "0".to_owned());
        let window_id = self.next_window_id;
        self.next_window_id += 1;
        let mut window = Window::new(window_id, window_index, window_name, size, pane);
        window.mode_keys = self.mode_keys;
        window.word_separators = self.word_separators.clone();
        window.synchronize_panes = self.synchronize_panes;
        self.sessions[session_index].windows.push(window);
        self.sessions[session_index]
            .windows
            .sort_by_key(|window| window.index);
        self.sessions[session_index].next_window_index = self.sessions[session_index]
            .next_window_index
            .max(window_index.saturating_add(1));
        if !detached {
            self.sessions[session_index].select_window(window_index);
        }
        self.sync_group_windows(session_index);
        self.reflow_session(session_id);
        if let Some(format) = format {
            let session = &self.sessions[session_index];
            let window = session
                .windows
                .iter()
                .find(|window| window.index == window_index)
                .ok_or_else(|| "new window no longer exists".to_owned())?;
            Ok(render_format(
                format,
                &[
                    ("session_name", session.name.clone()),
                    ("window_index", window.index.to_string()),
                    ("window_name", window.name.clone()),
                    ("window_panes", window.panes.len().to_string()),
                ],
            ))
        } else {
            Ok(format!("{session_id}:{window_index}"))
        }
    }

    fn split_window(
        &mut self,
        shared: &SharedState,
        target: Option<&str>,
        horizontal: bool,
        before: bool,
        full: bool,
        detached: bool,
        zoom: bool,
        empty: bool,
        split_size: Option<&str>,
        command: &[String],
        cwd: Option<&str>,
    ) -> CommandResult {
        let (session_index, window_index, target_pane) = self.resolve_pane_target(target)?;
        let session_id = self.sessions[session_index].id;
        if self.sessions[session_index].windows[window_index].zoomed {
            self.sessions[session_index].windows[window_index].zoomed = false;
            self.reflow_session(session_id);
        }
        let size = self.sessions[session_index].size;
        let session_cwd = self.sessions[session_index].cwd.clone();
        let window = &self.sessions[session_index].windows[window_index];
        let target_rect = window
            .pane(target_pane)
            .map(|pane| pane.rect)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        if (horizontal && target_rect.cols < 3) || (!horizontal && target_rect.rows < 3) {
            return Err("pane is too small to split".to_owned());
        }
        let pane_index = self.sessions[session_index].windows[window_index].next_pane_index;
        let pane_id = self.next_pane_id();
        let mut pane = self.new_pane(
            shared,
            pane_id,
            pane_index,
            size,
            command,
            cwd.or(session_cwd.as_deref()),
            empty,
        )?;
        let window = &mut self.sessions[session_index].windows[window_index];
        let requested_size =
            split_size.and_then(|value| parse_split_size(value, horizontal, target_rect));
        let first_size = requested_size.map(|new_size| {
            let available = if horizontal {
                target_rect.cols.saturating_sub(1)
            } else {
                target_rect.rows.saturating_sub(1)
            };
            if before {
                new_size.min(available)
            } else {
                available.saturating_sub(new_size)
            }
        });
        if !window.layout.split_with_size(
            target_pane,
            pane_id,
            if horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            before,
            full,
            first_size,
        ) {
            pane.pty.kill();
            return Err("target pane is not in the active layout".to_owned());
        }
        window.next_pane_index += 1;
        if full {
            pane.full_axis = Some(if horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            });
        }
        window.panes.push(pane);
        if !detached || zoom {
            window.last_pane = Some(window.active_pane);
            window.active_pane = pane_id;
        }
        if zoom {
            window.zoomed = true;
        }
        self.reflow_session(self.sessions[session_index].id);
        self.sync_group_windows(session_index);
        Ok(format!("%{pane_id}"))
    }

    fn new_pane(
        &mut self,
        shared: &SharedState,
        pane_id: u64,
        index: u32,
        size: Size,
        command: &[String],
        cwd: Option<&str>,
        empty: bool,
    ) -> Result<Pane, String> {
        let start_path = cwd.map(str::to_owned).or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });
        if empty {
            let mut pane = Pane::empty(pane_id, index, size).map_err(|error| error.to_string())?;
            pane.parser = Parser::new(size.rows.max(1), size.cols.max(1), self.history_limit);
            pane.current_path = start_path.clone();
            pane.start_path = start_path;
            return Ok(pane);
        }
        let terminal = self
            .global_options
            .get("default-terminal")
            .map(String::as_str);
        let pty = Pty::spawn(
            command,
            cwd.map(Path::new),
            size,
            terminal,
            &self.environment,
        )
        .map_err(|error| error.to_string())?;
        let reader = pty.reader().map_err(|error| error.to_string())?;
        let pid = pty.pid();
        let mut pane = Pane::new(
            pane_id,
            index,
            size,
            pty,
            command.join(" "),
            command.to_vec(),
        );
        pane.parser = Parser::new(size.rows.max(1), size.cols.max(1), self.history_limit);
        pane.current_path = start_path.clone();
        pane.start_path = start_path;
        spawn_reader(Arc::clone(shared), pane_id, pid, reader);
        Ok(pane)
    }

    fn attach_target(&mut self, target: Option<&str>, size: Size) -> Result<u64, String> {
        let session_index = self.resolve_session_index(target)?;
        let id = self.sessions[session_index].id;
        self.resize_session(id, size);
        Ok(id)
    }

    fn register_client(&mut self, target: Option<&str>, size: Size) -> Result<(u64, u64), String> {
        let session_id = self.attach_target(target, size)?;
        let client_id = self.next_client_id;
        self.next_client_id = self.next_client_id.saturating_add(1);
        self.clients.insert(
            client_id,
            AttachedClient {
                session_id,
                size,
                prefix_pending: false,
                key_buffer: Vec::new(),
                input_buffer: Vec::new(),
                root_key_buffer: Vec::new(),
                copy_key_buffer: Vec::new(),
                repeat_key: None,
                repeat_binding: None,
                mouse_buffer: Vec::new(),
                mouse_drag_button: None,
                mouse_slider_offset: None,
                mouse_resize: None,
                last_mouse_click: None,
                prompt: None,
                tree_mode: None,
                buffer_mode: None,
                client_mode: None,
                panes_mode: None,
            },
        );
        Ok((session_id, client_id))
    }

    fn handle_client_input(&mut self, client_id: u64, bytes: &[u8], shared: &SharedState) {
        let extended_keys = self
            .global_options
            .get("extended-keys")
            .is_some_and(|value| value != "off");
        let mut input = self
            .clients
            .get_mut(&client_id)
            .map(|client| std::mem::take(&mut client.input_buffer))
            .unwrap_or_default();
        input.extend_from_slice(bytes);
        let (decoded, pending) = if extended_keys {
            decode_extended_key_input(&input)
        } else {
            (input, Vec::new())
        };
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.input_buffer = pending;
        }
        if decoded.is_empty() {
            return;
        }
        let bytes = decoded.as_slice();
        let mut index = 0;
        while index < bytes.len() {
            if let Some(consumed) = self.feed_panes_mode(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_tree_mode(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_buffer_mode(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_client_mode(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_client_prompt(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_client_mouse(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_copy_key_binding(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_copy_mode_input(client_id, &bytes[index..]) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_root_key_binding(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            if let Some(consumed) = self.feed_repeat_binding(client_id, &bytes[index..], shared) {
                index += consumed;
                continue;
            }
            // The prefix-number keys are built-in window selectors in tmux,
            // rather than ordinary bindings. Consume them before the prefix
            // table forwards an unbound digit to the active pane.
            let numeric_window = self.clients.get(&client_id).and_then(|client| {
                (client.prefix_pending && client.key_buffer.is_empty())
                    .then_some((client.session_id, bytes[index]))
                    .filter(|(_, byte)| byte.is_ascii_digit())
                    .map(|(session_id, byte)| (session_id, u32::from(byte - b'0')))
            });
            if let Some((session_id, window_index)) = numeric_window {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.prefix_pending = false;
                    client.key_buffer.clear();
                    client.repeat_key = None;
                    client.repeat_binding = None;
                }
                let _ = self.select_window(&format!("${session_id}:{window_index}"));
                index += 1;
                continue;
            }
            let byte = bytes[index];
            let prefix_pending = self
                .clients
                .get(&client_id)
                .is_some_and(|client| client.prefix_pending);
            if byte == 0x1b && !prefix_pending {
                let consumed = terminal_input_sequence_len(&bytes[index..]);
                let session_id = self.clients.get(&client_id).map(|client| client.session_id);
                if let Some(session_id) = session_id {
                    self.write_active(session_id, &bytes[index..index + consumed]);
                    index += consumed;
                    continue;
                }
            }
            let Some(client) = self.clients.get_mut(&client_id) else {
                return;
            };
            if !client.prefix_pending {
                if self.prefix.as_slice() == [byte] {
                    client.prefix_pending = true;
                } else {
                    let session_id = client.session_id;
                    self.write_active(session_id, &[byte]);
                }
                index += 1;
                continue;
            }

            client.key_buffer.push(byte);
            let candidate = client.key_buffer.clone();
            let exact = self.bindings.get(&candidate).cloned();
            let partial = self
                .bindings
                .keys()
                .any(|binding| binding.starts_with(&candidate));
            if partial && exact.is_none() {
                index += 1;
                continue;
            }
            let session_id = client.session_id;
            client.prefix_pending = false;
            client.key_buffer.clear();
            if let Some(binding) = exact {
                if binding._repeat {
                    client.repeat_key = Some(candidate.clone());
                    client.repeat_binding = Some(binding.clone());
                } else {
                    client.repeat_key = None;
                    client.repeat_binding = None;
                }
                let _ = self.execute_bound_commands(client_id, session_id, binding, shared);
            } else {
                client.repeat_key = None;
                client.repeat_binding = None;
                let mut forwarded = self.prefix.clone();
                forwarded.extend_from_slice(&candidate);
                self.write_active(session_id, &forwarded);
            }
            index += 1;
        }
    }

    fn feed_repeat_binding(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let (session_id, key, binding) = {
            let client = self.clients.get(&client_id)?;
            (
                client.session_id,
                client.repeat_key.clone()?,
                client.repeat_binding.clone()?,
            )
        };
        if !bytes.starts_with(&key) {
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.repeat_key = None;
                client.repeat_binding = None;
            }
            return None;
        }
        let _ = self.execute_bound_commands(client_id, session_id, binding, shared);
        Some(key.len())
    }

    fn feed_copy_key_binding(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let table = self.copy_table_for_client(client_id)?;
        self.feed_key_table_binding(client_id, bytes, table, shared)
    }

    /// Dispatch ordinary attached-client keys through the built-in copy-mode
    /// table after configured table bindings have had first refusal. This is
    /// separate from `send-keys -X`: the latter names actions directly, while
    /// an attached client supplies terminal key bytes such as C-a or Up.
    fn feed_copy_mode_input(&mut self, client_id: u64, bytes: &[u8]) -> Option<usize> {
        let pane_id = self.copy_pane_for_client(client_id)?;
        let (actions, consumed) = copy_input_actions(self, pane_id, bytes);
        for (action, repeat) in actions {
            let _ = self.execute_copy_action(pane_id, action, repeat);
        }
        Some(consumed.max(1).min(bytes.len()))
    }

    fn feed_root_key_binding(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        if self.copy_table_for_client(client_id).is_some() {
            return None;
        }
        self.feed_key_table_binding(client_id, bytes, "root", shared)
    }

    fn copy_table_for_client(&self, client_id: u64) -> Option<&'static str> {
        let pane = self.copy_pane_for_client(client_id)?;
        let mode = self.find_pane(pane)?.copy_mode.as_ref()?;
        Some(match mode.keys {
            CopyModeKeys::Vi => "copy-mode-vi",
            CopyModeKeys::Emacs => "copy-mode",
        })
    }

    fn copy_pane_for_client(&self, client_id: u64) -> Option<u64> {
        let session_id = self.clients.get(&client_id)?.session_id;
        self.sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(Session::active_window)
            .and_then(Window::active)
            .and_then(|pane| pane.copy_mode.as_ref().map(|_| pane.id))
    }

    fn feed_key_table_binding(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        table: &str,
        shared: &SharedState,
    ) -> Option<usize> {
        let byte = *bytes.first()?;
        let (session_id, candidate, had_buffer) = {
            let client = self.clients.get_mut(&client_id)?;
            let buffer = if table == "root" {
                &mut client.root_key_buffer
            } else {
                &mut client.copy_key_buffer
            };
            let had_buffer = !buffer.is_empty();
            buffer.push(byte);
            (client.session_id, buffer.clone(), had_buffer)
        };
        let table_name = table.to_owned();
        let exact = self
            .table_bindings
            .get(&(table_name.clone(), candidate.clone()))
            .cloned();
        let partial = self.table_bindings.keys().any(|(binding_table, binding)| {
            binding_table == &table_name && binding.starts_with(&candidate)
        });
        if exact.is_none() && !partial {
            if !had_buffer {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if table == "root" {
                        client.root_key_buffer.clear();
                    } else {
                        client.copy_key_buffer.clear();
                    }
                }
                return None;
            }
            if let Some(client) = self.clients.get_mut(&client_id) {
                if table == "root" {
                    client.root_key_buffer.clear();
                } else {
                    client.copy_key_buffer.clear();
                }
            }
            self.write_active(session_id, &candidate);
            return Some(1);
        }
        if exact.is_none() {
            return Some(1);
        }
        if let Some(client) = self.clients.get_mut(&client_id) {
            if table == "root" {
                client.root_key_buffer.clear();
            } else {
                client.copy_key_buffer.clear();
            }
        }
        if let Some(binding) = exact {
            let _ = self.execute_bound_commands(client_id, session_id, binding, shared);
        }
        Some(1)
    }

    fn feed_client_mouse(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let mouse_enabled = self
            .global_options
            .get("mouse")
            .is_some_and(|value| parse_on_off(value).unwrap_or(false));
        let has_buffer = self
            .clients
            .get(&client_id)
            .is_some_and(|client| !client.mouse_buffer.is_empty());
        if !mouse_enabled && !has_buffer {
            return None;
        }

        let mut complete = None;
        let mut invalid = None;
        let mut consumed = 0;
        let session_id;
        {
            let Some(client) = self.clients.get_mut(&client_id) else {
                return Some(bytes.len().max(1));
            };
            session_id = client.session_id;
            while consumed < bytes.len() {
                if client.mouse_buffer.is_empty() && !bytes[consumed..].starts_with(b"\x1b[<") {
                    break;
                }
                let byte = bytes[consumed];
                client.mouse_buffer.push(byte);
                consumed += 1;
                let buffer = &client.mouse_buffer;
                let valid_prefix = buffer.len() < 3 || buffer.starts_with(b"\x1b[<");
                if !valid_prefix {
                    invalid = Some(std::mem::take(&mut client.mouse_buffer));
                    break;
                }
                if buffer.len() > 3 && matches!(byte, b'M' | b'm') {
                    let event = std::mem::take(&mut client.mouse_buffer);
                    complete = parse_sgr_mouse(&event);
                    if complete.is_none() {
                        invalid = Some(event);
                    }
                    break;
                }
            }
        }
        if let Some(bytes) = invalid {
            self.write_active(session_id, &bytes);
        } else if let Some((button, col, row, release)) = complete {
            if let Some((pane_id, local_col, local_row, encoding)) =
                self.mouse_passthrough_target(session_id, col, row)
            {
                self.write_pane(
                    pane_id,
                    &encode_mouse_event(button, local_col, local_row, release, encoding),
                );
            } else {
                self.apply_mouse_event(client_id, session_id, button, col, row, release, shared);
            }
        }
        (consumed > 0).then_some(consumed)
    }

    /// Applications such as file managers and terminal editors can enable a
    /// mouse protocol of their own. When that state is active inside a pane,
    /// tm forwards the event with coordinates translated into the pane rather
    /// than turning it into a multiplexer action.
    fn mouse_passthrough_target(
        &self,
        session_id: u64,
        col: u16,
        row: u16,
    ) -> Option<(u64, u16, u16, vt100::MouseProtocolEncoding)> {
        let session = self.sessions.iter().find(|session| session.id == session_id)?;
        let window = session.active_window()?;
        let x = col.saturating_sub(1);
        let y = row.saturating_sub(1);
        let pane = window.panes.iter().find(|pane| {
            x >= pane.rect.x
                && x < pane.rect.x.saturating_add(pane.rect.cols)
                && y >= pane.rect.y
                && y < pane.rect.y.saturating_add(pane.rect.rows)
                && pane.parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
        })?;
        Some((
            pane.id,
            x.saturating_sub(pane.rect.x).saturating_add(1),
            y.saturating_sub(pane.rect.y).saturating_add(1),
            pane.parser.screen().mouse_protocol_encoding(),
        ))
    }

    fn apply_mouse_event(
        &mut self,
        client_id: u64,
        session_id: u64,
        button: u16,
        col: u16,
        row: u16,
        release: bool,
        shared: &SharedState,
    ) {
        let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let Some(window) = session.active_window() else {
            return;
        };
        let x = col.saturating_sub(1);
        let y = row.saturating_sub(1);
        let motion = button & 0x20 != 0;
        let base_button = button & 0x03;

        // A border is a real interactive cell even though it is not part of
        // either pane rectangle. Keep the exact split path captured on the
        // initial press so nested same-axis layouts resize the border that was
        // grabbed, not merely the first matching split around the pane.
        if let Some(resize) = self
            .clients
            .get(&client_id)
            .and_then(|client| client.mouse_resize.clone())
        {
            if base_button != resize.button {
                return;
            }
            if release {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.mouse_resize = None;
                    client.mouse_drag_button = None;
                }
                return;
            }
            if motion {
                let coordinate = match resize.axis {
                    Axis::Horizontal => x,
                    Axis::Vertical => y,
                };
                let delta = i32::from(coordinate) - i32::from(resize.start_coordinate);
                let desired = i32::from(resize.start_size)
                    .saturating_add(delta)
                    .clamp(0, i32::from(u16::MAX)) as u16;
                self.resize_mouse_separator(session_id, resize, desired);
                return;
            }
        }

        if !window.zoomed
            && !release
            && !motion
            && base_button == 0
            && let Some(separator) = window.layout.separator_at(
                Rect {
                    x: 0,
                    y: 0,
                    cols: window.size.cols,
                    rows: window.size.rows,
                },
                x,
                y,
            )
        {
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.mouse_drag_button = Some(base_button);
                client.mouse_resize = Some(MouseResize {
                    button: base_button,
                    axis: separator.axis,
                    path: separator.path,
                    start_coordinate: separator.coordinate,
                    start_size: separator.first_size,
                });
            }
            return;
        }

        let Some(pane) = window.panes.iter().find(|pane| {
            x >= pane.rect.x
                && x < pane.rect.x.saturating_add(pane.rect.cols)
                && y >= pane.rect.y
                && y < pane.rect.y.saturating_add(pane.rect.rows)
        }) else {
            return;
        };
        let pane_id = pane.id;
        let pane_in_copy_mode = pane.copy_mode.is_some();
        let local_row = usize::from(y.saturating_sub(pane.rect.y));
        let local_col = usize::from(x.saturating_sub(pane.rect.x));
        let scrollbar_hit = self.copy_mode_scrollbar_hit(window, pane, local_col, local_row);
        let dragging_button = self
            .clients
            .get(&client_id)
            .and_then(|client| client.mouse_drag_button);
        let dragging = dragging_button == Some(base_button);
        if motion && !release {
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.mouse_drag_button = Some(base_button);
            }
        } else if release
            && dragging_button.is_some()
            && let Some(client) = self.clients.get_mut(&client_id)
        {
            client.mouse_drag_button = None;
            client.mouse_slider_offset = None;
        }
        if let Some(hit) = scrollbar_hit {
            if !motion && !release && matches!(hit, CopyScrollbarHit::Slider) {
                let slider_offset = self.copy_mode_scrollbar_slider_offset(pane, local_row);
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.mouse_drag_button = Some(base_button);
                    client.mouse_slider_offset = slider_offset;
                }
            }
            if !release {
                let action = if motion && dragging {
                    let slider_offset = self
                        .clients
                        .get(&client_id)
                        .and_then(|client| client.mouse_slider_offset)
                        .unwrap_or(0);
                    CopyAction::ScrollToMouse(Some(local_row.saturating_sub(slider_offset)))
                } else {
                    match hit {
                        CopyScrollbarHit::BeforeSlider => CopyAction::PageUp,
                        CopyScrollbarHit::Slider => {
                            let slider_offset = self
                                .clients
                                .get(&client_id)
                                .and_then(|client| client.mouse_slider_offset)
                                .unwrap_or(0);
                            CopyAction::ScrollToMouse(Some(local_row.saturating_sub(slider_offset)))
                        }
                        CopyScrollbarHit::AfterSlider => CopyAction::PageDown,
                    }
                };
                if pane_in_copy_mode {
                    let _ = self.execute_copy_action(pane_id, action, 1);
                } else if !motion {
                    let target = format!("%{pane_id}");
                    let _ = self.enter_copy_mode(Some(&target), None, false, false, false, false);
                    let _ = self.execute_copy_action(pane_id, action, 1);
                }
            }
            return;
        }
        let click_count = if !release && button < 64 {
            let now = Instant::now();
            let base_button = button & 0x03;
            let count = self
                .clients
                .get(&client_id)
                .and_then(|client| client.last_mouse_click)
                .filter(|last| {
                    last.button == base_button
                        && last.col == col
                        && last.row == row
                        && now.duration_since(last.at) <= Duration::from_millis(500)
                })
                .map_or(1, |last| if last.count >= 3 { 1 } else { last.count + 1 });
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.last_mouse_click = Some(MouseClickState {
                    button: base_button,
                    col,
                    row,
                    count,
                    at: now,
                });
            }
            count
        } else {
            1
        };
        let binding_name = if release && dragging {
            (base_button <= 2).then(|| format!("MouseDragEnd{}Pane", base_button + 1))
        } else {
            mouse_binding_name(button, release, click_count)
        };
        let table = if pane.copy_mode.is_some() {
            if matches!(window.mode_keys, CopyModeKeys::Vi) {
                "copy-mode-vi"
            } else {
                "copy-mode"
            }
        } else {
            "root"
        };
        if let Some(binding_name) = binding_name.as_deref()
            && let Some(binding) = self
                .mouse_bindings
                .get(&(table.to_owned(), binding_name.to_owned()))
                .or_else(|| {
                    self.mouse_bindings
                        .get(&("root".to_owned(), binding_name.to_owned()))
                })
                .cloned()
        {
            let context = self.mouse_context_for_pane(pane, local_row, local_col, button);
            self.mouse_context = Some(context);
            let result = self.execute_bound_commands(client_id, session_id, binding, shared);
            self.mouse_context = None;
            let _ = result;
            return;
        }
        if !release && !motion && base_button == 0 && (click_count == 2 || click_count >= 3) {
            let target = format!("%{pane_id}");
            if self
                .find_pane(pane_id)
                .is_some_and(|pane| pane.copy_mode.is_none())
            {
                let _ = self.enter_copy_mode(Some(&target), None, false, true, false, false);
            }
            let history_limit = self.history_limit;
            if let Some(pane) = self.find_pane_mut(pane_id) {
                let mut source_parser = copy_source_parser(pane, history_limit);
                if let Some(mode) = pane.copy_mode.as_mut() {
                    let parser = source_parser
                        .as_mut()
                        .map_or(&mut pane.parser, |parser| parser);
                    mode.mouse_position(parser, local_row, local_col, false, false);
                }
            }
            let action = if click_count == 2 {
                CopyAction::SelectWord
            } else {
                CopyAction::SelectLine
            };
            let _ = self.execute_copy_action(pane_id, action, 1);
            let _ = self.execute_copy_action(
                pane_id,
                CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                1,
            );
            return;
        }
        if button == 64 || button == 65 {
            if self
                .find_pane(pane_id)
                .is_some_and(|pane| pane.copy_mode.is_none())
            {
                let target = format!("%{pane_id}");
                let _ = self.enter_copy_mode(Some(&target), None, true, false, false, false);
            }
            let action = if button == 64 {
                CopyAction::ScrollUp
            } else {
                CopyAction::ScrollDown
            };
            let _ = self.execute_copy_action(pane_id, action, 5);
            return;
        }
        if !release && !motion && base_button == 1 && !pane_in_copy_mode {
            let target = format!("%{pane_id}");
            let _ = self.select_pane(Some(&target), PaneDirection::Last, None, None, None);
            let _ = self.paste_buffer(Some(&target), None, false, true, None, false);
            return;
        }
        if base_button != 0 {
            return;
        }
        if motion && !release && !pane_in_copy_mode {
            let target = format!("%{pane_id}");
            let _ = self.enter_copy_mode(Some(&target), None, false, false, false, false);
        }
        if release && dragging {
            let history_limit = self.history_limit;
            if let Some(pane) = self.find_pane_mut(pane_id) {
                let mut source_parser = copy_source_parser(pane, history_limit);
                if let Some(mode) = pane.copy_mode.as_mut() {
                    let parser = source_parser
                        .as_mut()
                        .map_or(&mut pane.parser, |parser| parser);
                    mode.mouse_position(parser, local_row, local_col, false, true);
                }
            }
            let _ = self.execute_copy_action(
                pane_id,
                CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                1,
            );
            return;
        }
        if !motion {
            let target = format!("%{pane_id}");
            let _ = self.select_pane(Some(&target), PaneDirection::Last, None, None, None);
        }
        let history_limit = self.history_limit;
        if let Some(pane) = self.find_pane_mut(pane_id) {
            let mut source_parser = copy_source_parser(pane, history_limit);
            if let Some(mode) = pane.copy_mode.as_mut() {
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut pane.parser, |parser| parser);
                if !motion && !release && pane_in_copy_mode {
                    mode.mouse_down_position(parser, local_row, local_col);
                } else {
                    mode.mouse_position(
                        parser,
                        local_row,
                        local_col,
                        motion && !dragging && !release,
                        release,
                    );
                }
            }
        }
    }

    fn copy_mode_scrollbar_hit(
        &self,
        window: &Window,
        pane: &Pane,
        local_col: usize,
        local_row: usize,
    ) -> Option<CopyScrollbarHit> {
        let state = window
            .options
            .get("pane-scrollbars")
            .or_else(|| self.global_options.get("pane-scrollbars"))
            .map(String::as_str)
            .unwrap_or("off");
        let visible = match state {
            "on" => true,
            "modal" | "auto-hide" => pane.copy_mode.is_some(),
            _ => false,
        };
        if !visible {
            return None;
        }
        let position = window
            .options
            .get("pane-scrollbars-position")
            .or_else(|| self.global_options.get("pane-scrollbars-position"))
            .map(String::as_str)
            .unwrap_or("right");
        let width = window
            .options
            .get("pane-scrollbars-style")
            .or_else(|| self.global_options.get("pane-scrollbars-style"))
            .and_then(|style| {
                style.split(',').find_map(|part| {
                    part.trim()
                        .strip_prefix("width=")
                        .and_then(|value| value.parse::<usize>().ok())
                })
            })
            .unwrap_or(1)
            .max(1)
            .min(usize::from(pane.rect.cols));
        let in_scrollbar = if position == "left" {
            local_col < width
        } else {
            local_col.saturating_add(width) >= usize::from(pane.rect.cols)
        };
        if !in_scrollbar {
            return None;
        }
        let mut parser =
            copy_source_parser(pane, self.history_limit).unwrap_or_else(|| parser_for_pane(pane));
        if let Some(mode) = pane.copy_mode.as_ref() {
            Some(mode.scrollbar_hit(&mut parser, local_row))
        } else {
            let (history, live) = history_rows(&mut parser);
            Some(scrollbar_hit_for(
                history.len().saturating_add(live.len()),
                usize::from(parser.screen().size().0),
                history.len(),
                0,
                local_row,
            ))
        }
    }

    fn copy_mode_scrollbar_slider_offset(&self, pane: &Pane, row: usize) -> Option<usize> {
        let mut parser =
            copy_source_parser(pane, self.history_limit).unwrap_or_else(|| parser_for_pane(pane));
        if let Some(mode) = pane.copy_mode.as_ref() {
            return mode.scrollbar_slider_offset(&mut parser, row);
        }
        let (history, live) = history_rows(&mut parser);
        let total_rows = history.len().saturating_add(live.len());
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        let max_scroll = total_rows.saturating_sub(viewport_rows);
        let (slider_top, slider_rows) =
            scrollbar_geometry_for(total_rows, viewport_rows, max_scroll, 0);
        (row >= slider_top && row < slider_top.saturating_add(slider_rows))
            .then_some(row.saturating_sub(slider_top))
    }

    fn mouse_context_for_pane(
        &self,
        pane: &Pane,
        local_row: usize,
        local_col: usize,
        button: u16,
    ) -> MouseContext {
        let (line, separators) = if let Some(mode) = pane.copy_mode.as_ref() {
            let mut parser =
                copy_source_parser(pane, 10_000).unwrap_or_else(|| parser_for_pane(pane));
            let (history, live) = history_rows(&mut parser);
            let total_rows = history.len().saturating_add(live.len()).max(1);
            let viewport_rows = usize::from(parser.screen().size().0).max(1);
            let viewport_start = total_rows
                .saturating_sub(viewport_rows)
                .saturating_sub(mode.scroll_position());
            let line = history
                .into_iter()
                .chain(live)
                .nth(viewport_start.saturating_add(local_row))
                .unwrap_or_default();
            (line, mode.word_separators().to_owned())
        } else {
            let line = pane
                .parser
                .screen()
                .rows(0, pane.rect.cols)
                .nth(local_row)
                .unwrap_or_default();
            (line, DEFAULT_WORD_SEPARATORS.to_owned())
        };
        let raw_output = pane
            .copy_source
            .as_ref()
            .map_or(pane.raw_output.as_slice(), |source| {
                source.raw_output.as_slice()
            });
        let hyperlink = mouse_hyperlink_at(raw_output, local_row, local_col, pane.rect);
        let word_column = display_column_to_char_index(&line, local_col);
        MouseContext {
            x: local_col,
            y: local_row,
            pane_id: pane.id,
            word: mouse_word_at(&line, word_column, &separators),
            line,
            hyperlink,
            button: (button & 0x03).saturating_add(1),
        }
    }

    fn feed_tree_mode(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let Some(client) = self.clients.get(&client_id) else {
            return Some(bytes.len().max(1));
        };
        let Some(mode) = client.tree_mode.as_ref() else {
            return None;
        };
        let Some(&byte) = bytes.first() else {
            return Some(0);
        };
        if mode.confirmation.is_some() {
            if matches!(byte, b'y' | b'Y') {
                self.confirm_tree_entry(client_id, true);
            } else if matches!(byte, b'n' | b'N' | 0x1b | b'q') {
                self.confirm_tree_entry(client_id, false);
            }
            return Some(1);
        }
        if mode.filter_input.is_some() {
            if bytes.starts_with(b"\x1b[A") || bytes.starts_with(b"\x1b[B") {
                return Some(3);
            }
            if byte == 0x1b {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.filter_input = None;
                }
                return Some(1);
            }
            if byte == b'\r' || byte == b'\n' {
                let filter = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                    .and_then(|mode| mode.filter_input.take())
                    .map(|input| String::from_utf8_lossy(&input).into_owned())
                    .unwrap_or_default();
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.filter = (!filter.is_empty()).then_some(filter);
                    mode.cursor = 0;
                }
                self.rebuild_tree_mode(client_id);
                return Some(1);
            }
            if matches!(byte, 0x08 | 0x7f) {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.pop();
                }
                return Some(1);
            }
            if byte >= b' ' {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.push(byte);
                }
                return Some(1);
            }
            return Some(1);
        }

        let (consumed, action) = if bytes.starts_with(b"\x1b[A") {
            (3, Some('k'))
        } else if bytes.starts_with(b"\x1b[B") {
            (3, Some('j'))
        } else {
            (1, Some(byte as char))
        };
        match action {
            Some('q') | Some('\x1b') => {
                let (source_pane, kill_on_exit) = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.tree_mode.as_ref())
                    .map(|mode| (mode.source_pane, mode.kill_on_exit))
                    .unwrap_or((None, false));
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.tree_mode = None;
                }
                if let (Some(source_pane), true) = (source_pane, kill_on_exit) {
                    let _ = self.kill_pane(Some(&format!("%{source_pane}")), false, None);
                }
            }
            Some('j') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.cursor = (mode.cursor + 1).min(mode.entries.len().saturating_sub(1));
                }
            }
            Some('k') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.cursor = mode.cursor.saturating_sub(1);
                }
            }
            Some('g') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.cursor = 0;
                }
            }
            Some('f') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.filter_input = Some(Vec::new());
                }
            }
            Some('c') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.tree_mode.as_mut())
                {
                    mode.filter = None;
                    mode.cursor = 0;
                }
                self.rebuild_tree_mode(client_id);
            }
            Some('x') => {
                let entry = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.tree_mode.as_ref())
                    .and_then(|mode| mode.entries.get(mode.cursor))
                    .cloned();
                if let Some(entry) = entry {
                    let label = self.tree_confirmation_label(&entry);
                    if let Some(mode) = self
                        .clients
                        .get_mut(&client_id)
                        .and_then(|client| client.tree_mode.as_mut())
                    {
                        mode.confirmation = Some(entry);
                        mode.confirmation_label = Some(label);
                    }
                }
            }
            Some('h') => {
                let key = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.tree_mode.as_ref())
                    .and_then(|mode| mode.entries.get(mode.cursor))
                    .map(|entry| entry.key);
                if let Some(key) = key {
                    if let Some(mode) = self
                        .clients
                        .get_mut(&client_id)
                        .and_then(|client| client.tree_mode.as_mut())
                    {
                        mode.collapsed.insert(key);
                    }
                    self.rebuild_tree_mode(client_id);
                }
            }
            Some('l') => {
                let key = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.tree_mode.as_ref())
                    .and_then(|mode| mode.entries.get(mode.cursor))
                    .map(|entry| entry.key);
                if let Some(key) = key {
                    if let Some(mode) = self
                        .clients
                        .get_mut(&client_id)
                        .and_then(|client| client.tree_mode.as_mut())
                    {
                        mode.collapsed.remove(&key);
                    }
                    self.rebuild_tree_mode(client_id);
                }
            }
            Some('\r') | Some('\n') => self.activate_tree_entry(client_id),
            _ => {}
        }
        let _ = shared;
        Some(consumed)
    }

    fn confirm_tree_entry(&mut self, client_id: u64, confirmed: bool) {
        let Some((entry, _)) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.tree_mode.as_mut())
            .and_then(|mode| Some((mode.confirmation.take()?, mode.confirmation_label.take())))
        else {
            return;
        };
        if !confirmed {
            return;
        }
        let result = if let Some(pane_id) = entry.pane_id {
            self.kill_pane(Some(&format!("%{pane_id}")), false, None)
        } else if let Some(window_id) = entry.window_id {
            self.kill_window(Some(&format!("@{window_id}")), false)
        } else {
            self.kill_session(Some(&format!("${}", entry.session_id)), false)
        };
        if result.is_ok() && self.clients.contains_key(&client_id) {
            self.rebuild_tree_mode(client_id);
        }
    }

    fn tree_confirmation_label(&self, entry: &TreeEntry) -> String {
        let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.id == entry.session_id)
        else {
            return "Kill item".to_owned();
        };
        if let Some(pane_id) = entry.pane_id {
            let index = session
                .windows
                .iter()
                .flat_map(|window| window.panes.iter())
                .find(|pane| pane.id == pane_id)
                .map(|pane| pane.index.to_string())
                .unwrap_or_else(|| pane_id.to_string());
            return format!("Kill pane {index}");
        }
        if let Some(window_id) = entry.window_id {
            let index = session
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.index.to_string())
                .unwrap_or_else(|| window_id.to_string());
            return format!("Kill window {index}");
        }
        format!("Kill session {}", session.name)
    }

    fn activate_tree_entry(&mut self, client_id: u64) {
        let Some((entry, source_pane, kill_on_exit)) = self
            .clients
            .get(&client_id)
            .and_then(|client| client.tree_mode.as_ref())
            .and_then(|mode| {
                mode.entries
                    .get(mode.cursor)
                    .cloned()
                    .map(|entry| (entry, mode.source_pane, mode.kill_on_exit))
            })
        else {
            return;
        };
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == entry.session_id)
        {
            if let Some(window_id) = entry.window_id
                && let Some(window) = session.windows.iter().find(|window| window.id == window_id)
            {
                session.active_window = window.index;
            }
            if let Some(pane_id) = entry.pane_id
                && let Some(window) = session.active_window()
                && window.panes.iter().any(|pane| pane.id == pane_id)
            {
                let window_index = window.index;
                if let Some(window) = session
                    .windows
                    .iter_mut()
                    .find(|window| window.index == window_index)
                {
                    window.active_pane = pane_id;
                }
            }
        }
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.session_id = entry.session_id;
            client.tree_mode = None;
        }
        if let (Some(source_pane), true) = (source_pane, kill_on_exit) {
            let _ = self.kill_pane(Some(&format!("%{source_pane}")), false, None);
        }
    }

    fn feed_buffer_mode(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let Some(client) = self.clients.get(&client_id) else {
            return Some(bytes.len().max(1));
        };
        let Some(mode) = client.buffer_mode.as_ref() else {
            return None;
        };
        let Some(&byte) = bytes.first() else {
            return Some(0);
        };
        if mode.filter_input.is_some() {
            if bytes.starts_with(b"\x1b[A") || bytes.starts_with(b"\x1b[B") {
                return Some(3);
            }
            if byte == 0x1b {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.filter_input = None;
                }
                return Some(1);
            }
            if byte == b'\r' || byte == b'\n' {
                let filter = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                    .and_then(|mode| mode.filter_input.take())
                    .map(|input| String::from_utf8_lossy(&input).into_owned())
                    .unwrap_or_default();
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.filter = (!filter.is_empty()).then_some(filter);
                    mode.cursor = 0;
                }
                self.rebuild_buffer_mode(client_id);
                return Some(1);
            }
            if matches!(byte, 0x08 | 0x7f) {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.pop();
                }
                return Some(1);
            }
            if byte >= b' ' {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.push(byte);
                }
                return Some(1);
            }
            return Some(1);
        }
        let (consumed, action) = if bytes.starts_with(b"\x1b[A") {
            (3, Some('k'))
        } else if bytes.starts_with(b"\x1b[B") {
            (3, Some('j'))
        } else {
            (1, Some(byte as char))
        };
        match action {
            Some('q') | Some('\x1b') => {
                let (source_pane, kill_on_exit) = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.buffer_mode.as_ref())
                    .map(|mode| (mode.source_pane, mode.kill_on_exit))
                    .unwrap_or((0, false));
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.buffer_mode = None;
                }
                if kill_on_exit {
                    let _ = self.kill_pane(Some(&format!("%{source_pane}")), false, None);
                }
            }
            Some('j') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.cursor = (mode.cursor + 1).min(mode.entries.len().saturating_sub(1));
                }
            }
            Some('k') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.cursor = mode.cursor.saturating_sub(1);
                }
            }
            Some('g') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.cursor = 0;
                }
            }
            Some('f') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.filter_input = Some(Vec::new());
                }
            }
            Some('c') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.filter = None;
                    mode.cursor = 0;
                }
                self.rebuild_buffer_mode(client_id);
            }
            Some('\x14') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.buffer_mode.as_mut())
                {
                    mode.tagged = mode
                        .entries
                        .iter()
                        .map(|entry| entry.name.clone())
                        .collect();
                }
            }
            Some('d') => self.delete_buffer_mode_entry(client_id),
            Some('D') => self.delete_tagged_buffer_mode_entries(client_id),
            Some('\r') | Some('\n') => self.activate_buffer_mode_entry(client_id),
            _ => {}
        }
        let _ = shared;
        Some(consumed)
    }

    fn delete_buffer_mode_entry(&mut self, client_id: u64) {
        let name = self
            .clients
            .get(&client_id)
            .and_then(|client| client.buffer_mode.as_ref())
            .and_then(|mode| mode.entries.get(mode.cursor))
            .map(|entry| entry.name.clone());
        if let Some(name) = name {
            let _ = self.delete_buffer(Some(&name));
            self.rebuild_buffer_mode(client_id);
        }
    }

    fn delete_tagged_buffer_mode_entries(&mut self, client_id: u64) {
        let names = self
            .clients
            .get(&client_id)
            .and_then(|client| client.buffer_mode.as_ref())
            .map(|mode| mode.tagged.clone())
            .unwrap_or_default();
        if names.is_empty() {
            return;
        }
        self.buffers.retain(|buffer| !names.contains(&buffer.name));
        if let Some(mode) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.buffer_mode.as_mut())
        {
            mode.tagged.clear();
        }
        self.rebuild_buffer_mode(client_id);
    }

    fn activate_buffer_mode_entry(&mut self, client_id: u64) {
        let Some((name, pane_id, kill_on_exit)) = self
            .clients
            .get(&client_id)
            .and_then(|client| client.buffer_mode.as_ref())
            .and_then(|mode| {
                mode.entries
                    .get(mode.cursor)
                    .map(|entry| (entry.name.clone(), mode.source_pane, mode.kill_on_exit))
            })
        else {
            return;
        };
        let _ = self.paste_buffer(
            Some(&format!("%{pane_id}")),
            Some(&name),
            false,
            false,
            None,
            false,
        );
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.buffer_mode = None;
        }
        if kill_on_exit {
            let _ = self.kill_pane(Some(&format!("%{pane_id}")), false, None);
        }
    }

    fn feed_client_mode(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let Some(client) = self.clients.get(&client_id) else {
            return Some(bytes.len().max(1));
        };
        let Some(mode) = client.client_mode.as_ref() else {
            return None;
        };
        let Some(&byte) = bytes.first() else {
            return Some(0);
        };
        if mode.filter_input.is_some() {
            if bytes.starts_with(b"\x1b[A") || bytes.starts_with(b"\x1b[B") {
                return Some(3);
            }
            if byte == 0x1b {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.filter_input = None;
                }
                return Some(1);
            }
            if byte == b'\r' || byte == b'\n' {
                let filter = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                    .and_then(|mode| mode.filter_input.take())
                    .map(|input| String::from_utf8_lossy(&input).into_owned())
                    .unwrap_or_default();
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.filter = (!filter.is_empty()).then_some(filter);
                    mode.cursor = 0;
                }
                self.rebuild_client_mode(client_id);
                return Some(1);
            }
            if matches!(byte, 0x08 | 0x7f) {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.pop();
                }
                return Some(1);
            }
            if byte >= b' ' {
                if let Some(input) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                    .and_then(|mode| mode.filter_input.as_mut())
                {
                    input.push(byte);
                }
                return Some(1);
            }
            return Some(1);
        }
        let (consumed, action) = if bytes.starts_with(b"\x1b[A") {
            (3, Some('k'))
        } else if bytes.starts_with(b"\x1b[B") {
            (3, Some('j'))
        } else {
            (1, Some(byte as char))
        };
        match action {
            Some('q') | Some('\x1b') => {
                let (source_pane, kill_on_exit) = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.client_mode.as_ref())
                    .map(|mode| (mode.source_pane, mode.kill_on_exit))
                    .unwrap_or((0, false));
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.client_mode = None;
                }
                if kill_on_exit {
                    let _ = self.kill_pane(Some(&format!("%{source_pane}")), false, None);
                }
            }
            Some('j') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.cursor = (mode.cursor + 1).min(mode.entries.len().saturating_sub(1));
                }
            }
            Some('k') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.cursor = mode.cursor.saturating_sub(1);
                }
            }
            Some('g') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.cursor = 0;
                }
            }
            Some('f') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.filter_input = Some(Vec::new());
                }
            }
            Some('c') => {
                if let Some(mode) = self
                    .clients
                    .get_mut(&client_id)
                    .and_then(|client| client.client_mode.as_mut())
                {
                    mode.filter = None;
                    mode.cursor = 0;
                }
                self.rebuild_client_mode(client_id);
            }
            Some('d') | Some('\r') | Some('\n') => self.activate_client_mode_entry(client_id),
            _ => {}
        }
        let _ = shared;
        Some(consumed)
    }

    fn activate_client_mode_entry(&mut self, client_id: u64) {
        let (selected, source_pane, kill_on_exit) = self
            .clients
            .get(&client_id)
            .and_then(|client| client.client_mode.as_ref())
            .map(|mode| {
                (
                    mode.entries.get(mode.cursor).map(|entry| entry.client_id),
                    mode.source_pane,
                    mode.kill_on_exit,
                )
            })
            .unwrap_or((None, 0, false));
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.client_mode = None;
        }
        if let Some(selected) = selected {
            let _ = self.detach_client(Some(&format!("client{selected}")), false);
        }
        if kill_on_exit {
            let _ = self.kill_pane(Some(&format!("%{source_pane}")), false, None);
        }
    }

    fn feed_client_prompt(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let Some(client) = self.clients.get_mut(&client_id) else {
            return Some(bytes.len().max(1));
        };
        let Some(prompt) = client.prompt.as_mut() else {
            return None;
        };
        let Some(&byte) = bytes.first() else {
            return Some(0);
        };

        let prompt_type = prompt.prompt_type.clone();
        let history = self
            .prompt_history
            .get(&prompt_type)
            .cloned()
            .unwrap_or_default();
        if prompt.quoted {
            prompt.quoted = false;
            prompt.insert(byte);
            return Some(1);
        }
        if matches!(
            prompt.mode,
            AttachedPromptMode::Single | AttachedPromptMode::Key
        ) {
            if byte == 0x1b {
                client.prompt = None;
                return Some(1);
            }
            prompt.input.clear();
            prompt.insert(byte);
            let input = prompt.current_input();
            let command = prompt.expanded_command();
            let session_id = client.session_id;
            client.prompt = None;
            self.record_prompt_history(&prompt_type, &input);
            for line in config::parse(&command) {
                let mut commands = vec![line.tokens];
                commands.extend(line.chained);
                let _ = self.execute_bound_commands(
                    client_id,
                    session_id,
                    ConfigBinding {
                        _repeat: false,
                        commands,
                    },
                    shared,
                );
            }
            return Some(1);
        }
        if prompt.mode == AttachedPromptMode::Numeric
            && byte != b'\r'
            && byte != b'\n'
            && !byte.is_ascii_digit()
        {
            let input = prompt.current_input();
            let command = prompt.expanded_command();
            let session_id = client.session_id;
            client.prompt = None;
            self.record_prompt_history(&prompt_type, &input);
            for line in config::parse(&command) {
                let mut commands = vec![line.tokens];
                commands.extend(line.chained);
                let _ = self.execute_bound_commands(
                    client_id,
                    session_id,
                    ConfigBinding {
                        _repeat: false,
                        commands,
                    },
                    shared,
                );
            }
            return Some(1);
        }
        if prompt.backspace_exit && prompt.input.is_empty() && matches!(byte, 0x08 | 0x7f) {
            client.prompt = None;
            return Some(1);
        }
        let escape = if bytes.starts_with(b"\x1b[D") {
            Some((3, 0))
        } else if bytes.starts_with(b"\x1b[C") {
            Some((3, 1))
        } else if bytes.starts_with(b"\x1b[A") {
            Some((3, 5))
        } else if bytes.starts_with(b"\x1b[B") {
            Some((3, 6))
        } else if bytes.starts_with(b"\x1b[1~") {
            Some((4, 2))
        } else if bytes.starts_with(b"\x1b[H") || bytes.starts_with(b"\x1bOH") {
            Some((3, 2))
        } else if bytes.starts_with(b"\x1b[4~") {
            Some((4, 3))
        } else if bytes.starts_with(b"\x1b[F") || bytes.starts_with(b"\x1bOF") {
            Some((3, 3))
        } else if bytes.starts_with(b"\x1b[3~") {
            Some((4, 4))
        } else {
            None
        };
        if let Some((consumed, action)) = escape {
            match action {
                0 => prompt.move_left(),
                1 => prompt.move_right(),
                2 => prompt.move_start(),
                3 => prompt.move_end(),
                4 => prompt.delete(),
                5 => prompt.history_up(&history),
                6 => prompt.history_down(&history),
                _ => unreachable!(),
            }
            return Some(consumed);
        }

        if byte == 0x1b {
            client.prompt = None;
            return Some(1);
        }

        let mut accepted = None;
        match byte {
            b'\r' | b'\n' => {
                if prompt.advance_prompt() {
                    return Some(1);
                }
                let input = prompt.current_input();
                accepted = Some((client.session_id, prompt.expanded_command(), input));
            }
            0x01 => prompt.move_start(),
            0x02 => prompt.move_left(),
            0x04 => prompt.delete(),
            0x05 => prompt.move_end(),
            0x06 => prompt.move_right(),
            0x08 | 0x7f => prompt.backspace(),
            0x0b => prompt.yank_buffer = prompt.kill_to_end(),
            0x15 => prompt.yank_buffer = prompt.kill_to_start(),
            0x16 => prompt.quoted = true,
            0x17 => prompt.yank_buffer = prompt.kill_word(),
            0x19 => prompt.yank(),
            0x09 if prompt.prompt_type == "command" => prompt.complete_command(),
            _ => prompt.insert(byte),
        }
        let incremental_update = if accepted.is_none() && prompt.incremental {
            Some((
                client.session_id,
                prompt.command.clone(),
                format!("={}", String::from_utf8_lossy(&prompt.input)),
            ))
        } else {
            None
        };
        if let Some((session_id, command, input)) = accepted {
            client.prompt = None;
            self.record_prompt_history(&prompt_type, &input);
            let lines = config::parse(&command);
            for line in lines {
                let mut commands = vec![line.tokens];
                commands.extend(line.chained);
                let _ = self.execute_bound_commands(
                    client_id,
                    session_id,
                    ConfigBinding {
                        _repeat: false,
                        commands,
                    },
                    shared,
                );
            }
        } else if let Some((session_id, template, input)) = incremental_update {
            self.execute_prompt_template(client_id, session_id, &template, &input, shared);
        }
        Some(1)
    }

    fn record_prompt_history(&mut self, prompt_type: &str, input: &str) {
        let history = self
            .prompt_history
            .entry(prompt_type.to_owned())
            .or_default();
        if input.is_empty() || history.last().is_some_and(|last| last == input) {
            return;
        }
        history.push(input.to_owned());
        const PROMPT_HISTORY_LIMIT: usize = 100;
        if history.len() > PROMPT_HISTORY_LIMIT {
            history.remove(0);
        }
    }

    fn prompt_type(command: &[String]) -> String {
        command
            .iter()
            .position(|value| value == "-T")
            .and_then(|index| command.get(index + 1))
            .cloned()
            .unwrap_or_else(|| "command".to_owned())
    }

    fn prompt_mode(command: &[String]) -> AttachedPromptMode {
        if command.iter().any(|value| value == "-1") {
            AttachedPromptMode::Single
        } else if command.iter().any(|value| value == "-N") {
            AttachedPromptMode::Numeric
        } else if command.iter().any(|value| value == "-k") {
            AttachedPromptMode::Key
        } else {
            AttachedPromptMode::Line
        }
    }

    fn prompt_initial_inputs(command: &[String]) -> Vec<Vec<u8>> {
        command
            .iter()
            .position(|value| value == "-I")
            .and_then(|index| command.get(index + 1))
            .map_or_else(
                || vec![Vec::new()],
                |value| {
                    value
                        .split(',')
                        .map(|input| input.as_bytes().to_vec())
                        .collect()
                },
            )
    }

    fn prompt_labels(command: &[String]) -> Vec<String> {
        let prompt = command
            .iter()
            .position(|value| value == "-p")
            .and_then(|index| command.get(index + 1))
            .cloned()
            .unwrap_or_else(|| "command: ".to_owned());
        prompt.split(',').map(str::to_owned).collect()
    }

    fn prompt_template(command: &[String]) -> String {
        let mut index = 1;
        while index < command.len() {
            match command[index].as_str() {
                "-p" | "-I" | "-T" => index = index.saturating_add(2),
                "-1" | "-N" | "-i" | "-k" | "-e" | "-P" => index += 1,
                value if value.starts_with('-') => index += 1,
                _ => return command[index].clone(),
            }
        }
        String::new()
    }

    fn execute_prompt_template(
        &mut self,
        client_id: u64,
        session_id: u64,
        template: &str,
        input: &str,
        shared: &SharedState,
    ) {
        let command = if template.is_empty() {
            input.to_owned()
        } else {
            template.replace("%%", input)
        };
        for line in config::parse(&command) {
            let mut commands = vec![line.tokens];
            commands.extend(line.chained);
            let _ = self.execute_bound_commands(
                client_id,
                session_id,
                ConfigBinding {
                    _repeat: false,
                    commands,
                },
                shared,
            );
        }
    }

    fn execute_bound_commands(
        &mut self,
        client_id: u64,
        session_id: u64,
        binding: ConfigBinding,
        shared: &SharedState,
    ) -> CommandResult {
        for raw_command in binding.commands {
            let command = raw_command
                .iter()
                .map(|value| self.expand_mouse_format(value))
                .collect::<Vec<_>>();
            if command.is_empty() {
                continue;
            }
            if command[0] == "__detach" {
                self.clients.remove(&client_id);
                return Ok(String::new());
            }
            if command[0] == "send-prefix" {
                self.write_active(session_id, &self.prefix.clone());
                continue;
            }
            if command[0] == "command-prompt" {
                if self
                    .clients
                    .get(&client_id)
                    .is_some_and(|client| client.prompt.is_some())
                {
                    continue;
                }
                let labels = Self::prompt_labels(&command);
                let label = labels
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "command: ".to_owned());
                let template = Self::prompt_template(&command);
                let initial_inputs = Self::prompt_initial_inputs(&command);
                let initial_input = initial_inputs.first().cloned().unwrap_or_default();
                let prompt_type = Self::prompt_type(&command);
                let history_index = self.prompt_history.get(&prompt_type).map_or(0, Vec::len);
                let incremental = command.iter().any(|value| value == "-i");
                let callback_template = template.clone();
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.prompt = Some(AttachedPrompt {
                        command: template,
                        prompt_type,
                        label,
                        labels,
                        initial_inputs,
                        current_prompt: 0,
                        accepted_inputs: Vec::new(),
                        cursor: initial_input.len(),
                        input: initial_input.clone(),
                        history_index,
                        quoted: false,
                        yank_buffer: Vec::new(),
                        mode: Self::prompt_mode(&command),
                        backspace_exit: command.iter().any(|value| value == "-e"),
                        incremental: command.iter().any(|value| value == "-i"),
                        pane: command.iter().any(|value| value == "-P"),
                    });
                }
                if incremental {
                    let input = format!("={}", String::from_utf8_lossy(&initial_input));
                    self.execute_prompt_template(
                        client_id,
                        session_id,
                        &callback_template,
                        &input,
                        shared,
                    );
                }
                continue;
            }
            if command[0] == "display" || command[0] == "display-message" {
                self.last_message = Some(
                    command
                        .iter()
                        .skip(1)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                continue;
            }
            if command[0] == "select-layout" {
                if let Some(layout) = command.get(1) {
                    self.select_layout(session_id, layout);
                }
                continue;
            }
            if command[0] == "display-panes" {
                let (target, source, no_zoom, no_mode, selection_command, kill_on_exit) =
                    parse_panes_mode_command(&command[1..])?;
                if no_mode {
                    if !selection_command.is_empty() {
                        self.execute_bound_commands(
                            client_id,
                            session_id,
                            ConfigBinding {
                                _repeat: false,
                                commands: vec![selection_command],
                            },
                            shared,
                        )?;
                    }
                } else {
                    self.enter_panes_mode(
                        client_id,
                        target.as_deref(),
                        source.as_deref(),
                        no_zoom,
                        (!selection_command.is_empty()).then_some(selection_command),
                        kill_on_exit,
                    )?;
                }
                continue;
            }
            if command[0] == "choose-tree" {
                let (target, format, filter, sort, reverse, hide_source, kill_on_exit) =
                    parse_tree_mode_command(&command[1..])?;
                self.enter_tree_mode(
                    client_id,
                    target.as_deref(),
                    format.as_deref(),
                    filter.as_deref(),
                    &sort,
                    reverse,
                    hide_source,
                    kill_on_exit,
                )?;
                continue;
            }
            if command[0] == "choose-buffer" {
                let (target, format, filter, sort, reverse, kill_on_exit) =
                    parse_buffer_mode_command(&command[1..])?;
                self.enter_buffer_mode(
                    client_id,
                    target.as_deref(),
                    format.as_deref(),
                    filter.as_deref(),
                    &sort,
                    reverse,
                    kill_on_exit,
                )?;
                continue;
            }
            if command[0] == "choose-client" {
                let (target, format, filter, kill_on_exit) =
                    parse_client_mode_command(&command[1..])?;
                self.enter_client_mode(
                    client_id,
                    target.as_deref(),
                    format.as_deref(),
                    filter.as_deref(),
                    kill_on_exit,
                )?;
                continue;
            }
            let mut command = command.clone();
            let current_path = self
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| {
                    session
                        .active_window()
                        .and_then(Window::active)
                        .and_then(|pane| pane.current_path.clone())
                        .or_else(|| session.cwd.clone())
                });
            if let Some(current_path) = current_path {
                for value in &mut command {
                    if value == "#{pane_current_path}" {
                        *value = current_path.clone();
                    }
                }
            }
            if !command.iter().any(|value| value == "-t")
                && let Some(target) = self
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| format!("{}:", session.name))
                && matches!(
                    command[0].as_str(),
                    "break-pane"
                        | "capture-pane"
                        | "clear-history"
                        | "copy-mode"
                        | "copy-mode-and-page"
                        | "kill-pane"
                        | "kill-window"
                        | "join-pane"
                        | "new-window"
                        | "next-window"
                        | "previous-window"
                        | "rename-window"
                        | "resize-pane"
                        | "respawn-pane"
                        | "respawn-window"
                        | "rotate-window"
                        | "select-pane"
                        | "send"
                        | "send-keys"
                        | "split-window"
                        | "swap-pane"
                        | "swap-window"
                )
            {
                command.extend(["-t".to_owned(), target]);
            }
            let invocation = crate::command::parse(&command)?;
            execute_request(self, shared, invocation.request)?;
        }
        Ok(String::new())
    }

    fn expand_mouse_format(&self, value: &str) -> String {
        let Some(mouse) = self.mouse_context.as_ref() else {
            return value.to_owned();
        };
        render_format(
            value,
            &[
                ("mouse_x", mouse.x.to_string()),
                ("mouse_y", mouse.y.to_string()),
                ("mouse_word", mouse.word.clone()),
                ("mouse_line", mouse.line.clone()),
                ("mouse_pane", format!("%{}", mouse.pane_id)),
                ("mouse_button", mouse.button.to_string()),
                ("mouse_all_flag", "0".to_owned()),
                ("mouse_any_flag", "1".to_owned()),
                ("mouse_sgr_flag", "1".to_owned()),
                ("mouse_standard_flag", "0".to_owned()),
                ("mouse_utf8_flag", "0".to_owned()),
                ("mouse_hyperlink", mouse.hyperlink.clone()),
                ("mouse_status_line", "0".to_owned()),
                ("mouse_status_range", "0".to_owned()),
            ],
        )
    }

    fn select_layout(&mut self, session_id: u64, layout: &str) {
        let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let Some(window) = session.active_window() else {
            return;
        };
        let pane_ids = window.panes.iter().map(|pane| pane.id).collect::<Vec<_>>();
        if pane_ids.len() < 2 {
            return;
        }
        let axis = match layout {
            "even-horizontal" => Axis::Horizontal,
            "even-vertical" => Axis::Vertical,
            _ => return,
        };
        let window = session
            .windows
            .iter_mut()
            .find(|window| window.index == session.active_window)
            .expect("active window exists");
        window.layout = crate::model::Layout::Leaf(pane_ids[0]);
        let mut target = pane_ids[0];
        for pane_id in pane_ids.into_iter().skip(1) {
            if !window
                .layout
                .split_with_size(target, pane_id, axis, false, false, None)
            {
                return;
            }
            // Keep extending the leaf just added. The active pane can be a
            // different pane when a layout is selected through a chained
            // binding, and using it here can leave the new pane unreachable
            // from the layout tree.
            target = pane_id;
        }
        self.reflow_session(session_id);
    }

    #[cfg(test)]
    fn execute_config_line(
        &mut self,
        client_id: u64,
        line: ConfigLine,
        shared: &SharedState,
    ) -> CommandResult {
        if line.tokens.is_empty() {
            return Ok(String::new());
        }
        match line.tokens[0].as_str() {
            "bind" | "bind-key" => {
                self.install_binding(&line.tokens, &line.chained)?;
            }
            "unbind" | "unbind-key" => {
                let mut table = "prefix".to_owned();
                let mut index = 1;
                while let Some(value) = line.tokens.get(index) {
                    match value.as_str() {
                        "-n" => table = "root".to_owned(),
                        "-T" => {
                            index += 1;
                            if let Some(name) = line.tokens.get(index) {
                                table = name.clone();
                            }
                        }
                        _ => {}
                    }
                    index += 1;
                }
                if let Some(key_name) = line.tokens.last() {
                    if let Some(key) = config::key_bytes(key_name) {
                        if table == "prefix" {
                            self.bindings.remove(&key);
                        } else {
                            self.table_bindings.remove(&(table, key));
                        }
                    } else {
                        self.mouse_bindings.remove(&(table, key_name.to_owned()));
                    }
                }
            }
            "display" | "display-message" => {}
            "select-layout" => {
                if let Some(client) = self.clients.get(&client_id)
                    && let Some(layout) = line.tokens.get(1)
                {
                    self.select_layout(client.session_id, layout);
                }
            }
            "set" | "set-option" | "setw" | "set-window-option" => {
                self.apply_config_option(&line.tokens)?;
            }
            _ => {}
        }
        let _ = shared;
        Ok(String::new())
    }

    #[cfg(test)]
    fn install_binding(&mut self, tokens: &[String], chained: &[Vec<String>]) -> CommandResult {
        let mut index = 1;
        let mut repeat = false;
        let mut table = "prefix".to_owned();
        while let Some(value) = tokens.get(index) {
            match value.as_str() {
                "-r" => repeat = true,
                "-n" => table = "root".to_owned(),
                "-T" => {
                    index += 1;
                    table = tokens
                        .get(index)
                        .cloned()
                        .ok_or_else(|| "bind -T requires a key table".to_owned())?;
                }
                value if value.starts_with('-') && value.len() > 1 => {}
                _ => break,
            }
            index += 1;
        }
        let key_name = tokens
            .get(index)
            .ok_or_else(|| "bind requires a key".to_owned())?;
        let mut commands = Vec::new();
        if let Some(command) = tokens.get(index + 1..)
            && !command.is_empty()
        {
            commands.push(command.to_vec());
        }
        commands.extend(chained.iter().cloned());
        let binding = ConfigBinding {
            _repeat: repeat,
            commands,
        };
        if let Some(key) = config::key_bytes(key_name) {
            if table == "prefix" {
                self.bindings.insert(key, binding);
            } else {
                self.table_bindings.insert((table, key), binding);
            }
        } else if is_mouse_binding_name(key_name) {
            self.mouse_bindings
                .insert((table, key_name.to_owned()), binding);
        } else {
            return Err(format!("bind requires a supported key: {key_name}"));
        }
        Ok(String::new())
    }

    #[cfg(test)]
    fn apply_config_option(&mut self, tokens: &[String]) -> CommandResult {
        let mut index = 1;
        let mut append = false;
        while let Some(value) = tokens.get(index) {
            if !value.starts_with('-') {
                break;
            }
            append |= value.contains('a');
            index += 1;
        }
        let key = tokens
            .get(index)
            .ok_or_else(|| "configuration option requires a name".to_owned())?;
        let value = tokens.get(index + 1..).unwrap_or_default().join(" ");
        if append {
            let value = self
                .global_options
                .get(key)
                .map(|existing| format!("{existing},{}", value))
                .unwrap_or(value);
            return self.set_global_option(key, &value, false);
        }
        self.set_global_option(key, &value, false)
    }

    #[cfg(test)]
    fn apply_test_config(&mut self, contents: &str) {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        for line in config::parse(contents) {
            let _ = self.execute_config_line(0, line, &shared);
        }
    }

    fn resize_session(&mut self, session_id: u64, size: Size) {
        let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        session.size = size.bounded();
        for window in &mut session.windows {
            window.size = session.size;
            window.reflow();
            for pane in &pane_iter(window) {
                let _ = pane.pty.resize(pane.rect_size());
            }
        }
    }

    fn reflow_session(&mut self, session_id: u64) {
        let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        for window in &mut session.windows {
            window.size = session.size;
            window.reflow();
            for pane in &window.panes {
                let _ = pane.pty.resize(pane.rect_size());
            }
        }
    }

    fn write_active(&mut self, session_id: u64, bytes: &[u8]) {
        let pane_ids = if let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        {
            if let Some(window) = session.active_window() {
                if window.synchronize_panes {
                    window.panes.iter().map(|pane| pane.id).collect::<Vec<_>>()
                } else {
                    window
                        .active()
                        .map(|pane| vec![pane.id])
                        .unwrap_or_default()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        for pane_id in pane_ids {
            self.write_pane(pane_id, bytes);
        }
    }

    fn write_pane(&mut self, pane_id: u64, bytes: &[u8]) {
        let in_copy_mode = self
            .find_pane(pane_id)
            .is_some_and(|pane| pane.copy_mode.is_some());
        if in_copy_mode {
            let (actions, _) = copy_input_actions(self, pane_id, bytes);
            for (action, repeat) in actions {
                let _ = self.execute_copy_action(pane_id, action, repeat);
            }
        } else if let Some(pane) = self.find_pane_mut(pane_id) {
            let _ = pane.pty.write(bytes);
        }
    }

    fn enter_copy_mode(
        &mut self,
        target: Option<&str>,
        source: Option<&str>,
        exit_on_scroll: bool,
        hide_position: bool,
        kill_on_exit: bool,
        page: bool,
    ) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let source_snapshot = source
            .map(|source| {
                self.resolve_pane_target(Some(source))
                    .map(|(_, _, source_id)| source_id)
            })
            .transpose()?
            .filter(|source_id| *source_id != pane_id)
            .and_then(|source_id| {
                self.find_pane(source_id).map(|pane| CopySource {
                    raw_output: if pane.raw_output.is_empty() {
                        pane.parser.screen().contents().into_bytes()
                    } else {
                        pane.raw_output.clone()
                    },
                    history_floor: pane.history_floor,
                })
            });
        let pane_mode_clients = self
            .clients
            .iter()
            .filter(|(_, client)| {
                client
                    .panes_mode
                    .as_ref()
                    .is_some_and(|mode| mode.target_pane == pane_id)
            })
            .map(|(client_id, _)| *client_id)
            .collect::<Vec<_>>();
        for client_id in pane_mode_clients {
            self.exit_panes_mode(client_id);
        }
        let window = &self.sessions[session_index].windows[window_index];
        let keys = window.mode_keys;
        let word_separators = window.word_separators.clone();
        let wrap_search = window
            .options
            .get("wrap-search")
            .or_else(|| self.global_options.get("wrap-search"))
            .is_none_or(|value| parse_on_off(value).unwrap_or(true));
        let history_limit = self.history_limit;
        let line_number_mode = window
            .options
            .get("copy-mode-line-numbers")
            .or_else(|| self.global_options.get("copy-mode-line-numbers"))
            .map_or(CopyLineNumberMode::Off, |value| {
                parse_copy_line_number_mode(value)
            });
        let pane = self
            .find_pane_mut(pane_id)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        pane.panes_mode = false;
        let prompt_history = pane.copy_prompt_history.clone();
        pane.enter_copy_mode(
            keys,
            exit_on_scroll,
            kill_on_exit,
            hide_position,
            wrap_search,
            &word_separators,
            pane.history_floor,
            &prompt_history,
            source_snapshot,
            history_limit,
        );
        if let Some(mode) = pane.copy_mode.as_mut() {
            mode.set_line_number_mode(line_number_mode);
        }
        if page && let Some(pane) = self.find_pane_mut(pane_id) {
            let mut source_parser = copy_source_parser(pane, history_limit);
            if let Some(mode) = pane.copy_mode.as_mut() {
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut pane.parser, |parser| parser);
                let _ = mode.execute(parser, CopyAction::PageUp, 1);
            }
        }
        Ok(String::new())
    }

    fn enter_panes_mode(
        &mut self,
        client_id: u64,
        target: Option<&str>,
        source: Option<&str>,
        no_zoom: bool,
        command: Option<Vec<String>>,
        kill_on_exit: bool,
    ) -> CommandResult {
        if self
            .clients
            .get(&client_id)
            .and_then(|client| client.panes_mode.as_ref())
            .is_some()
        {
            self.exit_panes_mode(client_id);
        }
        let (target_session, target_window, target_pane) = if let Some(target) = target {
            self.resolve_pane_target(Some(target))?
        } else {
            let session_id = self
                .clients
                .get(&client_id)
                .ok_or_else(|| "client not found".to_owned())?
                .session_id;
            let session_index = self
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| "client session no longer exists".to_owned())?;
            let active_index = self.sessions[session_index].active_window;
            let window_index = self.sessions[session_index]
                .windows
                .iter()
                .position(|window| window.index == active_index)
                .ok_or_else(|| "client active window no longer exists".to_owned())?;
            let pane_id = self.sessions[session_index].windows[window_index].active_pane;
            (session_index, window_index, pane_id)
        };
        let (source_session, source_window, _) = source
            .map(|source| self.resolve_pane_target(Some(source)))
            .transpose()?
            .unwrap_or((target_session, target_window, target_pane));
        if source_session != target_session {
            return Err("display-panes source and target must share a session".to_owned());
        }
        let source_window_id = self.sessions[source_session].windows[source_window].id;
        let display_format = self.sessions[source_session].windows[source_window]
            .options
            .get("display-panes-format")
            .or_else(|| self.global_options.get("display-panes-format"))
            .map(String::as_str)
            .unwrap_or("#{pane_index}");
        let source_size = self.sessions[source_session].windows[source_window].size;
        let mut unzoomed_rects = HashMap::new();
        self.sessions[source_session].windows[source_window]
            .layout
            .rectangles(
                Rect {
                    x: 0,
                    y: 0,
                    cols: source_size.cols,
                    rows: source_size.rows,
                },
                &mut unzoomed_rects,
            );
        let entries = self.sessions[source_session].windows[source_window]
            .panes
            .iter()
            .map(|pane| {
                let unzoomed = unzoomed_rects.get(&pane.id).copied().unwrap_or(pane.rect);
                let values = [
                    ("pane_id", format!("%{}", pane.id)),
                    ("pane_index", pane.index.to_string()),
                    ("pane_width", unzoomed.cols.to_string()),
                    ("pane_height", unzoomed.rows.to_string()),
                    ("pane_unzoomed_width", unzoomed.cols.to_string()),
                    ("pane_unzoomed_height", unzoomed.rows.to_string()),
                    ("pane_title", pane.title.clone()),
                    ("window_id", format!("@{source_window_id}")),
                ];
                PaneDisplayEntry {
                    pane_id: pane.id,
                    index: pane.index,
                    text: render_status_styles(&render_format_with_options(
                        display_format,
                        &values,
                        &self.global_options,
                    )),
                }
            })
            .collect::<Vec<_>>();
        let previous_zoomed = self.sessions[target_session].windows[target_window].zoomed;
        if let Some(pane) = self.find_pane_mut(target_pane) {
            pane.panes_mode = true;
        }
        if !no_zoom {
            self.sessions[target_session].windows[target_window].zoomed = true;
            self.reflow_session(self.sessions[target_session].id);
        }
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.tree_mode = None;
            client.buffer_mode = None;
            client.client_mode = None;
            client.panes_mode = Some(PaneDisplayMode {
                entries,
                target_pane,
                previous_zoomed,
                command,
                kill_on_exit,
            });
        }
        Ok(String::new())
    }

    fn exit_panes_mode(&mut self, client_id: u64) {
        let Some(mode) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.panes_mode.take())
        else {
            return;
        };
        let Some((session_index, window_index, pane_id)) =
            self.find_pane_location(mode.target_pane)
        else {
            return;
        };
        if let Some(pane) = self.find_pane_mut(pane_id) {
            pane.panes_mode = false;
        }
        self.sessions[session_index].windows[window_index].zoomed = mode.previous_zoomed;
        self.reflow_session(self.sessions[session_index].id);
        if mode.kill_on_exit {
            let _ = self.kill_pane(Some(&format!("%{}", mode.target_pane)), false, None);
        }
    }

    fn activate_panes_mode_entry(&mut self, client_id: u64, pane_id: u64, shared: &SharedState) {
        let command = self
            .clients
            .get(&client_id)
            .and_then(|client| client.panes_mode.as_ref())
            .and_then(|mode| mode.command.clone());
        let session_id = self.clients.get(&client_id).map(|client| client.session_id);
        self.exit_panes_mode(client_id);
        if let Some(command) = command {
            let command = command
                .into_iter()
                .map(|value| value.replace("%%", &format!("%{pane_id}")))
                .collect();
            if let Some(session_id) = session_id {
                let _ = self.execute_bound_commands(
                    client_id,
                    session_id,
                    ConfigBinding {
                        _repeat: false,
                        commands: vec![command],
                    },
                    shared,
                );
            }
        } else {
            let _ = self.select_pane(
                Some(&format!("%{pane_id}")),
                PaneDirection::Last,
                None,
                None,
                None,
            );
        }
    }

    fn feed_panes_mode(
        &mut self,
        client_id: u64,
        bytes: &[u8],
        shared: &SharedState,
    ) -> Option<usize> {
        let Some(client) = self.clients.get(&client_id) else {
            return Some(bytes.len().max(1));
        };
        let Some(mode) = client.panes_mode.as_ref() else {
            return None;
        };
        let Some(&byte) = bytes.first() else {
            return Some(0);
        };
        if byte == 0x1b || byte == b'q' {
            self.exit_panes_mode(client_id);
        } else if byte.is_ascii_digit() {
            let index = u32::from(byte - b'0');
            if let Some(entry) = mode.entries.iter().find(|entry| entry.index == index) {
                self.activate_panes_mode_entry(client_id, entry.pane_id, shared);
            }
        }
        Some(1)
    }

    fn enter_tree_mode(
        &mut self,
        client_id: u64,
        target: Option<&str>,
        format: Option<&str>,
        filter: Option<&str>,
        sort: &str,
        reverse: bool,
        hide_source: bool,
        kill_on_exit: bool,
    ) -> CommandResult {
        let source_pane = target
            .map(|target| {
                self.resolve_pane_target(Some(target))
                    .map(|(_, _, pane)| pane)
            })
            .transpose()?;
        let sort = match sort {
            "name" => TreeSort::Name,
            "index" | "" => TreeSort::Index,
            value => return Err(format!("unknown choose-tree sort: {value}")),
        };
        let Some(client) = self.clients.get_mut(&client_id) else {
            return Err("client not found".to_owned());
        };
        client.tree_mode = Some(TreeMode {
            entries: Vec::new(),
            cursor: 0,
            filter: filter.map(str::to_owned),
            filter_input: None,
            format: format.unwrap_or_default().to_owned(),
            sort,
            reverse,
            hide_source,
            source_pane,
            collapsed: HashSet::new(),
            no_matches: false,
            confirmation: None,
            confirmation_label: None,
            kill_on_exit,
        });
        self.rebuild_tree_mode(client_id);
        Ok(String::new())
    }

    fn client_for_mode_target(&self, target: Option<&str>) -> Result<u64, String> {
        let session_id = if let Some(target) = target {
            let session_index = self.resolve_pane_target(Some(target))?.0;
            self.sessions[session_index].id
        } else {
            let client_id = self
                .clients
                .keys()
                .next()
                .copied()
                .ok_or_else(|| "chooser requires an attached client".to_owned())?;
            return Ok(client_id);
        };
        self.clients
            .iter()
            .find(|(_, client)| client.session_id == session_id)
            .map(|(client_id, _)| *client_id)
            .ok_or_else(|| "chooser target has no attached client".to_owned())
    }

    fn client_active_pane(&self, client_id: u64) -> Result<u64, String> {
        let session_id = self
            .clients
            .get(&client_id)
            .ok_or_else(|| "client not found".to_owned())?
            .session_id;
        let session = self
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "client session no longer exists".to_owned())?;
        session
            .active_window()
            .and_then(Window::active)
            .map(|pane| pane.id)
            .ok_or_else(|| "client has no active pane".to_owned())
    }

    fn enter_buffer_mode(
        &mut self,
        client_id: u64,
        target: Option<&str>,
        format: Option<&str>,
        filter: Option<&str>,
        sort: &str,
        reverse: bool,
        kill_on_exit: bool,
    ) -> CommandResult {
        let source_pane = target
            .map(|target| self.resolve_pane_target(Some(target)).map(|(_, _, id)| id))
            .transpose()?
            .unwrap_or(self.client_active_pane(client_id)?);
        let sort = match sort {
            "name" => TreeSort::Name,
            "index" | "" => TreeSort::Index,
            value => return Err(format!("unknown choose-buffer sort: {value}")),
        };
        let Some(client) = self.clients.get_mut(&client_id) else {
            return Err("client not found".to_owned());
        };
        client.tree_mode = None;
        client.client_mode = None;
        client.buffer_mode = Some(BufferMode {
            entries: Vec::new(),
            cursor: 0,
            filter: filter.map(str::to_owned),
            filter_input: None,
            format: format.unwrap_or_default().to_owned(),
            sort,
            reverse,
            no_matches: false,
            source_pane,
            tagged: HashSet::new(),
            kill_on_exit,
        });
        self.rebuild_buffer_mode(client_id);
        Ok(String::new())
    }

    fn rebuild_buffer_mode(&mut self, client_id: u64) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        let Some(mode) = client.buffer_mode.as_ref() else {
            return;
        };
        let (entries, no_matches) = self.build_buffer_entries(
            mode.filter.as_deref(),
            &mode.format,
            mode.sort,
            mode.reverse,
        );
        if let Some(mode) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.buffer_mode.as_mut())
        {
            mode.entries = entries;
            mode.no_matches = no_matches;
            mode.cursor = mode.cursor.min(mode.entries.len().saturating_sub(1));
        }
    }

    fn build_buffer_entries(
        &self,
        filter: Option<&str>,
        format: &str,
        sort: TreeSort,
        reverse: bool,
    ) -> (Vec<BufferEntry>, bool) {
        let mut matching = self
            .buffers
            .iter()
            .filter(|buffer| {
                filter.is_none_or(|filter| {
                    let value = render_format_with_options(
                        filter,
                        &buffer_values(buffer),
                        &self.global_options,
                    );
                    !value.is_empty() && value != "0" && value != "false"
                })
            })
            .map(|buffer| buffer.name.clone())
            .collect::<HashSet<_>>();
        let no_matches = filter.is_some() && matching.is_empty();
        if no_matches {
            matching.extend(self.buffers.iter().map(|buffer| buffer.name.clone()));
        }
        let mut buffers = self
            .buffers
            .iter()
            .filter(|buffer| matching.contains(&buffer.name))
            .collect::<Vec<_>>();
        if sort == TreeSort::Name {
            buffers.sort_by_key(|buffer| buffer.name.clone());
        }
        if reverse {
            buffers.reverse();
        }
        let entries = buffers
            .into_iter()
            .map(|buffer| BufferEntry {
                name: buffer.name.clone(),
                text: if format.is_empty() {
                    buffer.name.clone()
                } else {
                    format!(
                        "{}: {}",
                        buffer.name,
                        render_format_with_options(
                            format,
                            &buffer_values(buffer),
                            &self.global_options,
                        )
                    )
                },
            })
            .collect();
        (entries, no_matches)
    }

    fn enter_client_mode(
        &mut self,
        client_id: u64,
        target: Option<&str>,
        format: Option<&str>,
        filter: Option<&str>,
        kill_on_exit: bool,
    ) -> CommandResult {
        if !self.clients.contains_key(&client_id) {
            return Err("client not found".to_owned());
        }
        let source_pane = target
            .map(|target| self.resolve_pane_target(Some(target)).map(|(_, _, id)| id))
            .transpose()?
            .unwrap_or(self.client_active_pane(client_id)?);
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.tree_mode = None;
            client.buffer_mode = None;
            client.client_mode = Some(ClientMode {
                entries: Vec::new(),
                cursor: 0,
                filter: filter.map(str::to_owned),
                filter_input: None,
                format: format.unwrap_or_default().to_owned(),
                no_matches: false,
                source_pane,
                kill_on_exit,
            });
        }
        self.rebuild_client_mode(client_id);
        Ok(String::new())
    }

    fn rebuild_client_mode(&mut self, client_id: u64) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        let Some(mode) = client.client_mode.as_ref() else {
            return;
        };
        let (entries, no_matches) = self.build_client_entries(mode.filter.as_deref(), &mode.format);
        if let Some(mode) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.client_mode.as_mut())
        {
            mode.entries = entries;
            mode.no_matches = no_matches;
            mode.cursor = mode.cursor.min(mode.entries.len().saturating_sub(1));
        }
    }

    fn build_client_entries(&self, filter: Option<&str>, format: &str) -> (Vec<ClientEntry>, bool) {
        let mut clients = self.clients.iter().collect::<Vec<_>>();
        clients.sort_by_key(|(id, _)| **id);
        let mut matching = clients
            .iter()
            .filter_map(|(id, client)| {
                let values = self.client_mode_values(**id, client)?;
                let matches = filter.is_none_or(|filter| {
                    let value = render_format_with_options(filter, &values, &self.global_options);
                    !value.is_empty() && value != "0" && value != "false"
                });
                matches.then_some(**id)
            })
            .collect::<HashSet<_>>();
        let no_matches = filter.is_some() && matching.is_empty();
        if no_matches {
            matching.extend(clients.iter().map(|(id, _)| **id));
        }
        let entries = clients
            .into_iter()
            .filter_map(|(id, client)| {
                if !matching.contains(id) {
                    return None;
                }
                let values = self.client_mode_values(*id, client)?;
                let session_name = values
                    .iter()
                    .find(|(name, _)| *name == "client_session")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default();
                let text = if format.is_empty() {
                    format!("client{id}: {session_name}")
                } else {
                    format!(
                        "client{id}: {}",
                        render_format_with_options(format, &values, &self.global_options)
                    )
                };
                Some(ClientEntry {
                    client_id: *id,
                    text,
                })
            })
            .collect();
        (entries, no_matches)
    }

    fn client_mode_values(
        &self,
        id: u64,
        client: &AttachedClient,
    ) -> Option<Vec<(&'static str, String)>> {
        let session = self
            .sessions
            .iter()
            .find(|session| session.id == client.session_id)?;
        Some(vec![
            ("client_id", id.to_string()),
            ("client_name", format!("client{id}")),
            ("client_session", session.name.clone()),
            ("client_session_id", format!("${}", session.id)),
            ("client_width", client.size.cols.to_string()),
            ("client_height", client.size.rows.to_string()),
            ("client_termname", "tm".to_owned()),
            ("client_control_mode", "0".to_owned()),
        ])
    }

    fn rebuild_tree_mode(&mut self, client_id: u64) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        let Some(mode) = client.tree_mode.as_ref() else {
            return;
        };
        let filter = mode.filter.clone();
        let format = mode.format.clone();
        let sort = mode.sort;
        let reverse = mode.reverse;
        let hide_source = mode.hide_source;
        let source_pane = mode.source_pane;
        let collapsed = mode.collapsed.clone();
        let (entries, no_matches) = self.build_tree_entries(
            filter.as_deref(),
            &format,
            sort,
            reverse,
            hide_source,
            source_pane,
            &collapsed,
        );
        if let Some(mode) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.tree_mode.as_mut())
        {
            mode.entries = entries;
            mode.no_matches = no_matches;
            mode.cursor = mode.cursor.min(mode.entries.len().saturating_sub(1));
        }
    }

    fn build_tree_entries(
        &self,
        filter: Option<&str>,
        format: &str,
        sort: TreeSort,
        reverse: bool,
        hide_source: bool,
        source_pane: Option<u64>,
        collapsed: &HashSet<TreeKey>,
    ) -> (Vec<TreeEntry>, bool) {
        let mut matching = HashSet::new();
        let mut any_match = filter.is_none();
        for session in &self.sessions {
            for window in &session.windows {
                for pane in &window.panes {
                    let matches = filter.is_none_or(|filter| {
                        self.tree_filter_matches(filter, session, window, pane)
                    });
                    if matches {
                        any_match = true;
                        matching.insert(pane.id);
                    }
                }
            }
        }
        let no_matches = filter.is_some() && !any_match;
        if no_matches {
            for session in &self.sessions {
                for window in &session.windows {
                    for pane in &window.panes {
                        matching.insert(pane.id);
                    }
                }
            }
        }

        let mut session_indices = (0..self.sessions.len()).collect::<Vec<_>>();
        if sort == TreeSort::Name {
            session_indices.sort_by_key(|index| self.sessions[*index].name.clone());
        }
        if reverse {
            session_indices.reverse();
        }
        let mut entries = Vec::new();
        for session_index in session_indices {
            let session = &self.sessions[session_index];
            let mut window_indices = session.windows.iter().enumerate().collect::<Vec<_>>();
            if sort == TreeSort::Name {
                window_indices.sort_by_key(|(_, window)| window.name.clone());
            }
            if reverse {
                window_indices.reverse();
            }
            let mut session_has_match = false;
            for (_, window) in &window_indices {
                if window.panes.iter().any(|pane| matching.contains(&pane.id)) {
                    session_has_match = true;
                    break;
                }
            }
            if !session_has_match {
                continue;
            }
            let session_key = TreeKey::Session(session.id);
            entries.push(TreeEntry {
                session_id: session.id,
                window_id: None,
                pane_id: None,
                key: session_key,
                text: self.tree_entry_text(session, None, None, format, 0),
            });
            if collapsed.contains(&session_key) {
                continue;
            }
            for (_, window) in window_indices {
                let matching_panes = window
                    .panes
                    .iter()
                    .filter(|pane| matching.contains(&pane.id))
                    .collect::<Vec<_>>();
                if matching_panes.is_empty() {
                    continue;
                }
                let window_key = TreeKey::Window(window.id);
                entries.push(TreeEntry {
                    session_id: session.id,
                    window_id: Some(window.id),
                    pane_id: None,
                    key: window_key,
                    text: self.tree_entry_text(session, Some(window), None, format, 1),
                });
                if collapsed.contains(&window_key) {
                    continue;
                }
                for pane in matching_panes {
                    if hide_source && source_pane == Some(pane.id) {
                        continue;
                    }
                    entries.push(TreeEntry {
                        session_id: session.id,
                        window_id: Some(window.id),
                        pane_id: Some(pane.id),
                        key: window_key,
                        text: self.tree_entry_text(session, Some(window), Some(pane), format, 2),
                    });
                }
            }
        }
        (entries, no_matches)
    }

    fn tree_filter_matches(
        &self,
        filter: &str,
        session: &Session,
        window: &Window,
        pane: &Pane,
    ) -> bool {
        let values = self.tree_values(session, window, pane);
        let value = render_format_with_options(filter, &values, &self.global_options);
        !value.is_empty() && value != "0" && value != "false"
    }

    fn tree_entry_text(
        &self,
        session: &Session,
        window: Option<&Window>,
        pane: Option<&Pane>,
        format: &str,
        depth: usize,
    ) -> String {
        let value = if format.is_empty() {
            match (window, pane) {
                (None, _) => session.name.clone(),
                (Some(window), None) => format!("{}: {}", window.index, window.name),
                (Some(window), Some(pane)) => format!("{}.{}", window.index, pane.index),
            }
        } else {
            let window = window.or_else(|| session.windows.first());
            let pane = pane.or_else(|| window.and_then(|window| window.panes.first()));
            match (window, pane) {
                (Some(window), Some(pane)) => render_format_with_options(
                    format,
                    &self.tree_values(session, window, pane),
                    &self.global_options,
                ),
                _ => format.to_owned(),
            }
        };
        format!("{}{}", "  ".repeat(depth), value)
    }

    fn tree_values<'a>(
        &self,
        session: &'a Session,
        window: &'a Window,
        pane: &'a Pane,
    ) -> Vec<(&'static str, String)> {
        vec![
            ("session_id", format!("${}", session.id)),
            ("session_name", session.name.clone()),
            ("session_windows", session.windows.len().to_string()),
            ("window_id", format!("@{}", window.id)),
            ("window_index", window.index.to_string()),
            ("window_name", window.name.clone()),
            ("window_panes", window.panes.len().to_string()),
            ("pane_id", format!("%{}", pane.id)),
            ("pane_index", pane.index.to_string()),
            ("pane_width", pane.rect.cols.to_string()),
            ("pane_height", pane.rect.rows.to_string()),
            (
                "pane_current_path",
                pane.current_path.clone().unwrap_or_default(),
            ),
            ("pane_title", pane.title.clone()),
            ("pane_dead", if pane.dead { "1" } else { "0" }.to_owned()),
        ]
    }

    fn execute_copy_action(
        &mut self,
        pane_id: u64,
        action: CopyAction,
        repeat: usize,
    ) -> CommandResult {
        if self.find_pane_location(pane_id).is_none() {
            return Err(format!("pane not found: %{pane_id}"));
        }
        let history_limit = self.history_limit;
        let (
            result,
            append,
            should_kill,
            pipe_command,
            store_buffer,
            buffer_prefix,
            set_paste,
            set_clipboard,
        ) = {
            let pane = self
                .find_pane_mut(pane_id)
                .ok_or_else(|| "target pane no longer exists".to_owned())?;
            let Some(mut mode) = pane.copy_mode.take() else {
                return Err("pane is not in copy mode".to_owned());
            };
            let mut source_parser = copy_source_parser(pane, history_limit);
            let parser = source_parser
                .as_mut()
                .map_or(&mut pane.parser, |parser| parser);
            let action_result = mode.execute(parser, action, repeat);
            pane.copy_prompt_history = mode.prompt_history().to_vec();
            let result = action_result.copied;
            let append = action_result.append;
            let should_kill = action_result.kill_pane;
            let pipe_command = action_result.pipe_command;
            let store_buffer = action_result.store_buffer;
            let buffer_prefix = action_result.buffer_prefix;
            let set_paste = action_result.set_paste;
            let set_clipboard = action_result.set_clipboard;
            let refresh_now = action_result.refresh_now;
            let live_raw_output = refresh_now
                .then(|| pane.copy_source.is_none().then(|| pane.raw_output.clone()))
                .flatten();
            if let Some(raw_output) = live_raw_output.as_deref() {
                mode.refresh_now(parser, raw_output);
            }
            if action_result.exit {
                parser.screen_mut().set_scrollback(0);
                pane.copy_source = None;
            } else {
                pane.copy_mode = Some(mode);
            }
            (
                result,
                append,
                should_kill,
                pipe_command,
                store_buffer,
                buffer_prefix,
                set_paste,
                set_clipboard,
            )
        };
        if let Some(data) = result {
            let data = data.into_bytes();
            if set_clipboard
                && self
                    .global_options
                    .get("set-clipboard")
                    .is_some_and(|value| matches!(value.as_str(), "on" | "external"))
            {
                self.clipboard_pending = Some(data.clone());
            }
            if let Some(command) = pipe_command.map(|command| {
                if command.is_empty() {
                    self.global_options
                        .get("copy-command")
                        .cloned()
                        .unwrap_or_default()
                } else {
                    command
                }
            }) {
                if store_buffer && set_paste {
                    self.store_copy_buffer(buffer_prefix, data.clone());
                }
                if !command.is_empty() {
                    run_copy_pipe(&command, &data, &self.environment)?;
                }
            } else if append {
                self.append_buffer(data);
            } else if set_paste {
                self.store_copy_buffer(buffer_prefix, data);
            }
        }
        if should_kill {
            let target = format!("%{pane_id}");
            self.kill_pane(Some(&target), false, None)?;
        }
        Ok(String::new())
    }

    fn display_message(&self, target: Option<&str>, format: &str) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let session = &self.sessions[session_index];
        let window = &session.windows[window_index];
        let pane = window
            .pane(pane_id)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        let format = self.expand_context_loops(format, session_index, window_index, pane_id);
        let session_names = self
            .sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>()
            .join("\0");
        let window_names = session
            .windows
            .iter()
            .map(|window| window.name.as_str())
            .collect::<Vec<_>>()
            .join("\0");
        Ok(format_pane(
            session,
            window,
            pane,
            self.marked_pane == Some(pane.id),
            self.marked_pane.is_some(),
            pane.copy_mode.as_ref(),
            self.window_link_count(window.id),
            self.session_group_info(session_index),
            &session_names,
            &window_names,
            &self.global_options,
            &format,
        ))
    }

    fn store_buffer(&mut self, name: Option<String>, data: Vec<u8>, append: bool) {
        let automatic = name.is_none();
        let name = name.unwrap_or_else(|| {
            let name = format!("buffer{}", self.next_buffer_id);
            self.next_buffer_id += 1;
            name
        });
        if let Some(buffer) = self.buffers.iter_mut().find(|buffer| buffer.name == name) {
            if append {
                buffer.data.extend_from_slice(&data);
            } else {
                buffer.data = data;
            }
            return;
        }
        self.buffers.insert(
            0,
            Buffer {
                name,
                data,
                automatic,
                created: unix_time(),
            },
        );
        let mut automatic_count = 0;
        self.buffers.retain(|buffer| {
            if !buffer.automatic {
                return true;
            }
            automatic_count += 1;
            automatic_count <= self.buffer_limit
        });
    }

    fn store_copy_buffer(&mut self, prefix: Option<String>, data: Vec<u8>) {
        if let Some(prefix) = prefix {
            let name = loop {
                let name = format!("{prefix}{}", self.next_buffer_id);
                self.next_buffer_id += 1;
                if !self.buffers.iter().any(|buffer| buffer.name == name) {
                    break name;
                }
            };
            self.buffers.insert(
                0,
                Buffer {
                    name,
                    data,
                    automatic: true,
                    created: unix_time(),
                },
            );
            let mut automatic_count = 0;
            self.buffers.retain(|buffer| {
                if !buffer.automatic {
                    return true;
                }
                automatic_count += 1;
                automatic_count <= self.buffer_limit
            });
        } else {
            self.store_buffer(None, data, false);
        }
    }

    fn append_buffer(&mut self, data: Vec<u8>) {
        if let Some(buffer) = self.buffers.iter_mut().find(|buffer| buffer.automatic) {
            let mut combined = data;
            combined.extend_from_slice(&buffer.data);
            buffer.data = combined;
        } else {
            self.store_buffer(None, data, false);
        }
    }

    fn show_buffer(&self, name: Option<&str>) -> CommandResult {
        self.buffers
            .iter()
            .find(|buffer| match name {
                Some(name) => buffer.name == name,
                None => buffer.automatic,
            })
            .map(|buffer| String::from_utf8_lossy(&buffer.data).into_owned())
            .ok_or_else(|| match name {
                Some(name) => format!("no buffer {name}"),
                None => "no buffers".to_owned(),
            })
    }

    fn rename_buffer(&mut self, name: Option<&str>, rename: &str) -> CommandResult {
        let name = name.ok_or_else(|| "set-buffer -n requires a buffer".to_owned())?;
        let buffer = self
            .buffers
            .iter_mut()
            .find(|buffer| buffer.name == name)
            .ok_or_else(|| format!("unknown buffer: {name}"))?;
        buffer.name = rename.to_owned();
        Ok(String::new())
    }

    fn list_buffers(&self, format: Option<&str>, filter: Option<&str>) -> CommandResult {
        let format = format.unwrap_or("#{buffer_name}: #{buffer_size} bytes: #{buffer_sample}");
        let lines = self
            .buffers
            .iter()
            .filter(|buffer| buffer_filter(buffer, filter))
            .map(|buffer| format_buffer(buffer, format, &self.global_options))
            .collect::<Vec<_>>();
        Ok(lines.join("\n"))
    }

    fn delete_buffer(&mut self, name: Option<&str>) -> CommandResult {
        let index = self
            .buffers
            .iter()
            .position(|buffer| match name {
                Some(name) => buffer.name == name,
                None => buffer.automatic,
            })
            .ok_or_else(|| match name {
                Some(name) => format!("unknown buffer: {name}"),
                None => "no buffer".to_owned(),
            })?;
        self.buffers.remove(index);
        Ok(String::new())
    }

    fn paste_buffer(
        &mut self,
        target: Option<&str>,
        name: Option<&str>,
        raw: bool,
        bracketed: bool,
        separator: Option<&[u8]>,
        delete: bool,
    ) -> CommandResult {
        let buffer_data = self
            .buffers
            .iter()
            .find(|buffer| match name {
                Some(name) => buffer.name == name,
                None => buffer.automatic,
            })
            .map(|buffer| buffer.data.clone())
            .ok_or_else(|| match name {
                Some(name) => format!("no buffer {name}"),
                None => "no buffer paste".to_owned(),
            })?;
        let mut data = Vec::with_capacity(buffer_data.len());
        if raw {
            data.extend_from_slice(&buffer_data);
        } else {
            let separator = separator.unwrap_or(b"\r");
            let mut start = 0;
            for (index, byte) in buffer_data.iter().enumerate() {
                if *byte == b'\n' {
                    data.extend_from_slice(&buffer_data[start..index]);
                    data.extend_from_slice(separator);
                    start = index + 1;
                }
            }
            data.extend_from_slice(&buffer_data[start..]);
        }
        if bracketed {
            let mut wrapped = Vec::with_capacity(data.len() + 16);
            wrapped.extend_from_slice(b"\x1b[200~");
            wrapped.extend_from_slice(&data);
            wrapped.extend_from_slice(b"\x1b[201~");
            data = wrapped;
        }
        let (_, _, pane_id) = self.resolve_pane_target(target)?;
        let pane = self
            .find_pane(pane_id)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        pane.pty.write(&data).map_err(|error| error.to_string())?;
        if delete {
            self.delete_buffer(name)?;
        }
        Ok(String::new())
    }

    fn load_buffer(&mut self, name: Option<String>, data: Vec<u8>) -> CommandResult {
        self.store_buffer(name, data, false);
        Ok(String::new())
    }

    fn save_buffer(&self, name: Option<&str>, path: Option<&str>, append: bool) -> CommandResult {
        let buffer = self
            .buffers
            .iter()
            .find(|buffer| match name {
                Some(name) => buffer.name == name,
                None => buffer.automatic,
            })
            .ok_or_else(|| match name {
                Some(name) => format!("no buffer {name}"),
                None => "no buffers".to_owned(),
            })?;
        if let Some(path) = path.filter(|path| *path != "-") {
            let mut options = fs::OpenOptions::new();
            options.create(true).write(true);
            if append {
                options.append(true);
            } else {
                options.truncate(true);
            }
            let mut file = options.open(path).map_err(|error| error.to_string())?;
            file.write_all(&buffer.data)
                .map_err(|error| error.to_string())?;
            Ok(String::new())
        } else {
            Ok(String::from_utf8_lossy(&buffer.data).into_owned())
        }
    }

    fn set_option(
        &mut self,
        target: Option<&str>,
        scope: Option<OptionScope>,
        key: &str,
        value: &str,
        unset: bool,
    ) -> CommandResult {
        if let Some(scope) = scope {
            match scope {
                OptionScope::Global => return self.set_global_option(key, value, unset),
                OptionScope::Session => {
                    let session_index = self.resolve_session_index(target)?;
                    if unset {
                        self.unset_session_option(session_index, key);
                    } else {
                        if key == "base-index" {
                            let base = value
                                .parse::<u32>()
                                .map_err(|_| "base-index requires an integer".to_owned())?;
                            self.sessions[session_index].base_index = base;
                            self.sessions[session_index].next_window_index = self.sessions
                                [session_index]
                                .next_window_index
                                .max(base.saturating_add(1));
                        } else if key == "renumber-windows" {
                            self.sessions[session_index].renumber_windows = parse_on_off(value)?;
                            if self.sessions[session_index].renumber_windows {
                                renumber_session_windows(&mut self.sessions[session_index]);
                            }
                        }
                        self.sessions[session_index]
                            .options
                            .insert(key.to_owned(), value.to_owned());
                        if key == "mode-keys" || key == "word-separators" {
                            self.apply_session_window_option(session_index, key, value)?;
                        }
                    }
                    return Ok(String::new());
                }
                OptionScope::Window => {
                    if unset {
                        return self.unset_window_option(target, key);
                    }
                    let (session_index, window_index) = self.resolve_window_target(target)?;
                    self.sessions[session_index].windows[window_index]
                        .options
                        .insert(key.to_owned(), value.to_owned());
                    if key == "mode-keys" || key == "word-separators" {
                        return self.set_window_option(target, key, value);
                    }
                    return Ok(String::new());
                }
                OptionScope::Pane => {
                    let (_, _, pane_id) = self.resolve_pane_target(target)?;
                    let pane = self
                        .find_pane_mut(pane_id)
                        .ok_or_else(|| "target pane no longer exists".to_owned())?;
                    if unset {
                        pane.options.remove(key);
                    } else {
                        pane.options.insert(key.to_owned(), value.to_owned());
                    }
                    return Ok(String::new());
                }
            }
        }
        // With no explicit scope, tmux infers the option's owning object.
        // Window options are the small exception to the session default; this
        // keeps `set mode-keys ...` and `set base-index ...` useful without a
        // target while preserving explicit -g/-s/-w/-p requests above.
        if target.is_none() {
            if matches!(key, "mode-keys" | "word-separators") {
                if unset {
                    return self.unset_window_option(None, key);
                }
                return self.set_window_option(None, key, value);
            }
            let session_index = self.resolve_session_index(None)?;
            if matches!(key, "base-index" | "renumber-windows") {
                if unset {
                    self.unset_session_option(session_index, key);
                    return Ok(String::new());
                }
                if key == "base-index" {
                    let base = value
                        .parse::<u32>()
                        .map_err(|_| "base-index requires an integer".to_owned())?;
                    self.sessions[session_index].base_index = base;
                    self.sessions[session_index].next_window_index = self.sessions[session_index]
                        .next_window_index
                        .max(base.saturating_add(1));
                } else {
                    self.sessions[session_index].renumber_windows = parse_on_off(value)?;
                    if self.sessions[session_index].renumber_windows {
                        renumber_session_windows(&mut self.sessions[session_index]);
                    }
                }
                self.sessions[session_index]
                    .options
                    .insert(key.to_owned(), value.to_owned());
                return Ok(String::new());
            }
            if unset {
                self.sessions[session_index].options.remove(key);
            } else {
                self.sessions[session_index]
                    .options
                    .insert(key.to_owned(), value.to_owned());
            }
            return Ok(String::new());
        }
        if let Some(target) = target {
            if unset {
                if matches!(key, "mode-keys" | "word-separators") {
                    return self.unset_window_option(Some(target), key);
                }
                let session_index = self.resolve_session_index(Some(target))?;
                self.unset_session_option(session_index, key);
                return Ok(String::new());
            }
            let (session_index, window_index) = self.resolve_window_target(Some(target))?;
            if key == "mode-keys" {
                let mode = match value {
                    "vi" => CopyModeKeys::Vi,
                    "emacs" => CopyModeKeys::Emacs,
                    _ => return Err(format!("unknown mode-keys value: {value}")),
                };
                self.sessions[session_index].windows[window_index].mode_keys = mode;
                self.sessions[session_index].windows[window_index]
                    .options
                    .insert(key.to_owned(), value.to_owned());
                return Ok(String::new());
            }
            if key == "word-separators" {
                self.sessions[session_index].windows[window_index].word_separators =
                    value.to_owned();
                self.sessions[session_index].windows[window_index]
                    .options
                    .insert(key.to_owned(), value.to_owned());
                return Ok(String::new());
            }
            if key == "synchronize-panes" {
                let enabled = parse_on_off(value)?;
                self.sessions[session_index].windows[window_index].synchronize_panes = enabled;
                self.sessions[session_index].windows[window_index]
                    .options
                    .insert(key.to_owned(), value.to_owned());
                return Ok(String::new());
            }
            if key == "base-index" {
                let base = value
                    .parse::<u32>()
                    .map_err(|_| "base-index requires an integer".to_owned())?;
                self.sessions[session_index].base_index = base;
                self.sessions[session_index].next_window_index = self.sessions[session_index]
                    .next_window_index
                    .max(base.saturating_add(1));
                return Ok(String::new());
            }
            if key == "renumber-windows" {
                self.sessions[session_index].renumber_windows = parse_on_off(value)?;
                if self.sessions[session_index].renumber_windows {
                    renumber_session_windows(&mut self.sessions[session_index]);
                }
                return Ok(String::new());
            }
            return Ok(String::new());
        }
        self.set_global_option(key, value, unset)
    }

    fn apply_session_window_option(
        &mut self,
        session_index: usize,
        key: &str,
        value: &str,
    ) -> CommandResult {
        if key == "mode-keys" {
            let mode = match value {
                "vi" => CopyModeKeys::Vi,
                "emacs" => CopyModeKeys::Emacs,
                _ => return Err(format!("unknown mode-keys value: {value}")),
            };
            for window in &mut self.sessions[session_index].windows {
                window.mode_keys = mode;
            }
        } else if key == "word-separators" {
            for window in &mut self.sessions[session_index].windows {
                window.word_separators = value.to_owned();
            }
        }
        Ok(String::new())
    }

    fn unset_session_option(&mut self, session_index: usize, key: &str) {
        self.sessions[session_index].options.remove(key);
        match key {
            "mode-keys" => {
                let mode = self.sessions[session_index]
                    .options
                    .get(key)
                    .or_else(|| self.global_options.get(key))
                    .map_or(CopyModeKeys::Emacs, |value| match value.as_str() {
                        "vi" => CopyModeKeys::Vi,
                        _ => CopyModeKeys::Emacs,
                    });
                for window in &mut self.sessions[session_index].windows {
                    window.mode_keys = mode;
                }
            }
            "word-separators" => {
                let separators = self.sessions[session_index]
                    .options
                    .get(key)
                    .or_else(|| self.global_options.get(key))
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_WORD_SEPARATORS.to_owned());
                for window in &mut self.sessions[session_index].windows {
                    window.word_separators = separators.clone();
                }
            }
            "base-index" => {
                let base = self.global_base_index();
                self.sessions[session_index].base_index = base;
                self.sessions[session_index].next_window_index = self.sessions[session_index]
                    .next_window_index
                    .max(base.saturating_add(1));
            }
            "renumber-windows" => {
                let enabled = self
                    .global_options
                    .get(key)
                    .is_some_and(|value| parse_on_off(value).unwrap_or(false));
                self.sessions[session_index].renumber_windows = enabled;
                if enabled {
                    renumber_session_windows(&mut self.sessions[session_index]);
                }
            }
            _ => {}
        }
    }

    fn unset_window_option(&mut self, target: Option<&str>, key: &str) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        self.sessions[session_index].windows[window_index]
            .options
            .remove(key);
        if key == "mode-keys" {
            let mode = self.sessions[session_index]
                .options
                .get(key)
                .or_else(|| self.global_options.get(key))
                .map_or(CopyModeKeys::Emacs, |value| match value.as_str() {
                    "vi" => CopyModeKeys::Vi,
                    _ => CopyModeKeys::Emacs,
                });
            self.sessions[session_index].windows[window_index].mode_keys = mode;
        } else if key == "word-separators" {
            let separators = self.sessions[session_index]
                .options
                .get(key)
                .or_else(|| self.global_options.get(key))
                .cloned()
                .unwrap_or_else(|| DEFAULT_WORD_SEPARATORS.to_owned());
            self.sessions[session_index].windows[window_index].word_separators = separators;
        } else if key == "synchronize-panes" {
            let enabled = self
                .global_options
                .get(key)
                .is_some_and(|value| parse_on_off(value).unwrap_or(false));
            self.sessions[session_index].windows[window_index].synchronize_panes = enabled;
        }
        Ok(String::new())
    }

    fn set_global_option(&mut self, key: &str, value: &str, unset: bool) -> CommandResult {
        if unset {
            self.global_options.remove(key);
            match key {
                "mode-keys" => {
                    self.mode_keys = CopyModeKeys::Emacs;
                    for session in &mut self.sessions {
                        let value = session
                            .options
                            .get(key)
                            .map_or(CopyModeKeys::Emacs, |value| match value.as_str() {
                                "vi" => CopyModeKeys::Vi,
                                _ => CopyModeKeys::Emacs,
                            });
                        for window in &mut session.windows {
                            window.mode_keys = value;
                        }
                    }
                }
                "word-separators" => {
                    self.word_separators = DEFAULT_WORD_SEPARATORS.to_owned();
                    for session in &mut self.sessions {
                        let value = session
                            .options
                            .get(key)
                            .cloned()
                            .unwrap_or_else(|| DEFAULT_WORD_SEPARATORS.to_owned());
                        for window in &mut session.windows {
                            window.word_separators = value.clone();
                        }
                    }
                }
                "synchronize-panes" => {
                    self.synchronize_panes = false;
                    for session in &mut self.sessions {
                        for window in &mut session.windows {
                            window.synchronize_panes = false;
                        }
                    }
                }
                "base-index" => {
                    for session in &mut self.sessions {
                        session.base_index = 0;
                    }
                }
                "renumber-windows" => {
                    for session in &mut self.sessions {
                        session.renumber_windows = false;
                    }
                }
                "remain-on-exit" => self.remain_on_exit = false,
                "buffer-limit" => self.buffer_limit = 50,
                "history-limit" => self.history_limit = 10_000,
                "prefix" => self.prefix = vec![2],
                _ => {}
            }
            return Ok(String::new());
        }
        if key == "prefix" {
            self.prefix = config::key_bytes(value)
                .ok_or_else(|| format!("unsupported prefix key: {value}"))?;
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "history-limit" {
            self.history_limit = value
                .parse::<usize>()
                .map_err(|_| "history-limit requires an integer".to_owned())?;
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "synchronize-panes" {
            let enabled = parse_on_off(value)?;
            self.synchronize_panes = enabled;
            for session in &mut self.sessions {
                for window in &mut session.windows {
                    window.synchronize_panes = enabled;
                }
            }
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "mode-keys" {
            self.mode_keys = match value {
                "vi" => CopyModeKeys::Vi,
                "emacs" => CopyModeKeys::Emacs,
                _ => return Err(format!("unknown mode-keys value: {value}")),
            };
            for session in &mut self.sessions {
                for window in &mut session.windows {
                    window.mode_keys = self.mode_keys;
                }
            }
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "word-separators" {
            self.word_separators = value.to_owned();
            for session in &mut self.sessions {
                for window in &mut session.windows {
                    window.word_separators = self.word_separators.clone();
                }
            }
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "base-index" {
            let base = value
                .parse::<u32>()
                .map_err(|_| "base-index requires an integer".to_owned())?;
            for session in &mut self.sessions {
                session.base_index = base;
                session.next_window_index = session.next_window_index.max(base.saturating_add(1));
            }
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "renumber-windows" {
            let enabled = parse_on_off(value)?;
            for session in &mut self.sessions {
                session.renumber_windows = enabled;
                if enabled {
                    renumber_session_windows(session);
                }
            }
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key == "remain-on-exit" {
            self.remain_on_exit = parse_on_off(value)?;
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        if key != "buffer-limit" {
            self.global_options.insert(key.to_owned(), value.to_owned());
            return Ok(String::new());
        }
        self.buffer_limit = value
            .parse::<usize>()
            .map_err(|_| "buffer-limit requires a positive integer".to_owned())?;
        while self
            .buffers
            .iter()
            .filter(|buffer| buffer.automatic)
            .count()
            > self.buffer_limit
        {
            if let Some(index) = self.buffers.iter().rposition(|buffer| buffer.automatic) {
                self.buffers.remove(index);
            } else {
                break;
            }
        }
        self.global_options.insert(key.to_owned(), value.to_owned());
        Ok(String::new())
    }

    fn show_options(
        &self,
        target: Option<&str>,
        global: bool,
        window: bool,
        pane: bool,
        value_only: bool,
        _all: bool,
        quiet: bool,
        key: Option<&str>,
    ) -> CommandResult {
        let mut options = self.global_options.clone();
        if key == Some("mode-keys") || (key.is_none() && global) {
            options.insert(
                "mode-keys".to_owned(),
                match self.mode_keys {
                    CopyModeKeys::Emacs => "emacs",
                    CopyModeKeys::Vi => "vi",
                }
                .to_owned(),
            );
        }
        if key == Some("word-separators") || (key.is_none() && global) {
            options.insert("word-separators".to_owned(), self.word_separators.clone());
        }
        if key == Some("buffer-limit") || (key.is_none() && global) {
            options.insert("buffer-limit".to_owned(), self.buffer_limit.to_string());
        }
        if key == Some("remain-on-exit") || (key.is_none() && global) {
            options.insert(
                "remain-on-exit".to_owned(),
                if self.remain_on_exit { "on" } else { "off" }.to_owned(),
            );
        }
        if key == Some("synchronize-panes") || (key.is_none() && global) {
            options.insert(
                "synchronize-panes".to_owned(),
                if self.synchronize_panes { "on" } else { "off" }.to_owned(),
            );
        }

        if !global && !window && !pane {
            let session_index = self.resolve_session_index(target)?;
            options.extend(self.sessions[session_index].options.clone());
        }

        if window || pane {
            let (session_index, window_index, pane_id) = if pane {
                self.resolve_pane_target(target)?
            } else {
                let (session_index, window_index) = self.resolve_window_target(target)?;
                let pane_id = self.sessions[session_index].windows[window_index].active_pane;
                (session_index, window_index, pane_id)
            };
            let session = &self.sessions[session_index];
            let current_window = &session.windows[window_index];
            let current_pane = current_window
                .pane(pane_id)
                .ok_or_else(|| "target pane no longer exists".to_owned())?;
            options.extend(session.options.clone());
            options.extend(current_window.options.clone());
            if pane {
                options.extend(current_pane.options.clone());
            }
            options.insert(
                "mode-keys".to_owned(),
                match current_window.mode_keys {
                    CopyModeKeys::Emacs => "emacs",
                    CopyModeKeys::Vi => "vi",
                }
                .to_owned(),
            );
            options.insert(
                "word-separators".to_owned(),
                current_window.word_separators.clone(),
            );
            options.insert(
                "synchronize-panes".to_owned(),
                if current_window.synchronize_panes {
                    "on"
                } else {
                    "off"
                }
                .to_owned(),
            );
            if pane {
                options.insert(
                    "pane-enabled".to_owned(),
                    if current_pane.enabled { "on" } else { "off" }.to_owned(),
                );
            }
        }

        if let Some(key) = key {
            let Some(option_value) = options.get(key) else {
                if quiet {
                    return Ok(String::new());
                }
                return Err(format!("invalid option: {key}"));
            };
            return Ok(if value_only {
                option_value.clone()
            } else {
                format!("{key} {}", quote_option_value(option_value))
            });
        }
        let mut names = options.into_iter().collect::<Vec<_>>();
        names.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(names
            .into_iter()
            .map(|(key, value)| {
                if value_only {
                    value
                } else {
                    format!("{key} {}", quote_option_value(&value))
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn set_environment(&mut self, name: &str, value: Option<&str>, remove: bool) -> CommandResult {
        if name.is_empty() || name.contains('=') || name.contains('\0') {
            return Err("invalid environment variable name".to_owned());
        }
        // The process environment itself is never mutated: `std::env::set_var`
        // is unsafe in edition 2024 because setenv races with getenv in a
        // multithreaded process, and rustix deliberately offers no replacement
        // (it is not a syscall). Instead `self.environment` is applied
        // explicitly to spawned children: pane PTYs receive it in
        // `Pty::spawn`, and each helper `Command::new("/bin/sh")` site passes
        // it via `.envs`, preserving tmux's "affects future spawned shells"
        // semantics.
        if remove {
            self.environment.remove(name);
        } else {
            let value = value.ok_or_else(|| "set-environment requires a value".to_owned())?;
            self.environment.insert(name.to_owned(), value.to_owned());
        }
        Ok(String::new())
    }

    fn show_environment(&self, format: Option<&str>, name: Option<&str>) -> CommandResult {
        if let Some(name) = name {
            let Some(value) = self.environment.get(name) else {
                return Err(format!("unknown environment variable: {name}"));
            };
            return Ok(format_environment(format, name, value));
        }
        let mut entries = self.environment.iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(right.0));
        Ok(entries
            .into_iter()
            .map(|(name, value)| format_environment(format, name, value))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn pipe_pane(
        &mut self,
        target: Option<&str>,
        command: Option<&str>,
        toggle: bool,
    ) -> CommandResult {
        let (_, _, pane_id) = self.resolve_pane_target(target)?;
        if toggle || command.is_none() {
            self.pane_pipes.remove(&pane_id);
            if toggle || command.is_none() {
                return Ok(String::new());
            }
        }
        let command = command.ok_or_else(|| "pipe-pane requires a shell command".to_owned())?;
        self.pane_pipes.remove(&pane_id);
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .envs(self.environment.iter())
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|error| format!("pipe-pane: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "pipe-pane could not open command stdin".to_owned())?;
        self.pane_pipes.insert(
            pane_id,
            PanePipe {
                stdin: Arc::new(Mutex::new(stdin)),
                child,
            },
        );
        Ok(String::new())
    }

    fn remove_exited_panes(&mut self, pane_ids: &HashSet<u64>) {
        let mut removed = Vec::new();
        let mut affected_sessions = HashSet::new();
        self.sessions.retain_mut(|session| {
            for window in &mut session.windows {
                let ids = window
                    .panes
                    .iter()
                    .filter(|pane| pane_ids.contains(&pane.id))
                    .map(|pane| pane.id)
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    affected_sessions.insert(session.id);
                }
                for id in &ids {
                    let _ = window.layout.remove(*id);
                }
                removed.extend(ids);
                window.panes.retain(|pane| !pane_ids.contains(&pane.id));
                for (index, pane) in window.panes.iter_mut().enumerate() {
                    pane.index = index as u32;
                }
                if !window.panes.is_empty()
                    && !window
                        .panes
                        .iter()
                        .any(|pane| pane.id == window.active_pane)
                {
                    window.active_pane = window.panes[0].id;
                }
            }
            session.windows.retain(|window| !window.panes.is_empty());
            if session.windows.is_empty() {
                false
            } else {
                if !session
                    .windows
                    .iter()
                    .any(|window| window.index == session.active_window)
                {
                    session.active_window = session.windows[0].index;
                }
                true
            }
        });
        for id in removed {
            self.pane_pipes.remove(&id);
        }
        // Pane exit can collapse a split without going through kill-pane.
        // Reflow every surviving window so its PTY receives the new geometry
        // and SIGWINCH just like an explicit pane close.
        for session_id in affected_sessions {
            self.reflow_session(session_id);
        }
    }

    fn set_window_option(&mut self, target: Option<&str>, key: &str, value: &str) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        if key == "mode-keys" {
            let mode = match value {
                "vi" => CopyModeKeys::Vi,
                "emacs" => CopyModeKeys::Emacs,
                _ => return Err(format!("unknown mode-keys value: {value}")),
            };
            self.sessions[session_index].windows[window_index].mode_keys = mode;
            self.sessions[session_index].windows[window_index]
                .options
                .insert(key.to_owned(), value.to_owned());
            Ok(String::new())
        } else if key == "window-size" {
            Ok(String::new())
        } else if key == "word-separators" {
            self.sessions[session_index].windows[window_index].word_separators = value.to_owned();
            self.sessions[session_index].windows[window_index]
                .options
                .insert(key.to_owned(), value.to_owned());
            Ok(String::new())
        } else if key == "synchronize-panes" {
            let enabled = parse_on_off(value)?;
            self.sessions[session_index].windows[window_index].synchronize_panes = enabled;
            self.sessions[session_index].windows[window_index]
                .options
                .insert(key.to_owned(), value.to_owned());
            Ok(String::new())
        } else {
            self.sessions[session_index].windows[window_index]
                .options
                .insert(key.to_owned(), value.to_owned());
            Ok(String::new())
        }
    }

    fn list_sessions(&self, format: Option<&str>) -> String {
        self.sessions
            .iter()
            .enumerate()
            .map(|(index, session)| match format {
                Some(format) => {
                    let (grouped, group_size, group_list) = self.session_group_info(index);
                    let format = self.expand_context_loops(format, index, 0, 0);
                    render_format_with_options(
                        &format,
                        &[
                            ("session_id", format!("${}", session.id)),
                            ("session_name", session.name.clone()),
                            ("session_windows", session.windows.len().to_string()),
                            ("session_attached", "0".to_owned()),
                            (
                                "session_grouped",
                                if grouped { "1" } else { "0" }.to_owned(),
                            ),
                            ("session_group_size", group_size.to_string()),
                            ("session_group_list", group_list),
                        ],
                        &self.global_options,
                    )
                }
                None => format!("{}: {} windows", session.name, session.windows.len()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn list_clients(&self, format: Option<&str>) -> CommandResult {
        let format = format.unwrap_or("#{client_name}: #{client_session}");
        let mut clients = self.clients.iter().collect::<Vec<_>>();
        clients.sort_by_key(|(id, _)| **id);
        Ok(clients
            .into_iter()
            .filter_map(|(id, client)| {
                let session = self
                    .sessions
                    .iter()
                    .find(|session| session.id == client.session_id)?;
                let values = [
                    ("client_id", id.to_string()),
                    ("client_name", format!("client{id}")),
                    ("client_session", session.name.clone()),
                    ("client_session_id", format!("${}", session.id)),
                    ("client_width", client.size.cols.to_string()),
                    ("client_height", client.size.rows.to_string()),
                    ("client_termname", "tm".to_owned()),
                    ("client_control_mode", "0".to_owned()),
                ];
                Some(render_format_with_options(
                    format,
                    &values,
                    &self.global_options,
                ))
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn resolve_client_ids(&self, target: Option<&str>) -> Vec<u64> {
        let mut ids = self.clients.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        if let Some(target) = target {
            let target = target.strip_prefix("client").unwrap_or(target);
            if let Ok(id) = target.parse::<u64>() {
                ids.retain(|candidate| *candidate == id);
            } else {
                ids.clear();
            }
        } else {
            ids.truncate(1);
        }
        ids
    }

    fn detach_client(&mut self, target: Option<&str>, all: bool) -> CommandResult {
        let ids = if all {
            self.clients.keys().copied().collect::<Vec<_>>()
        } else {
            self.resolve_client_ids(target)
        };
        if ids.is_empty() {
            return Err("no matching client".to_owned());
        }
        for id in ids {
            self.clients.remove(&id);
        }
        Ok(String::new())
    }

    fn switch_client(&mut self, client: Option<&str>, session: &str) -> CommandResult {
        let session_index = self.resolve_session_index(Some(session))?;
        let session_id = self.sessions[session_index].id;
        let client_id = self
            .resolve_client_ids(client)
            .first()
            .copied()
            .ok_or_else(|| "no matching client".to_owned())?;
        let size = self
            .clients
            .get(&client_id)
            .map(|client| client.size)
            .unwrap_or(Size::new(80, 24));
        if let Some(attached) = self.clients.get_mut(&client_id) {
            attached.session_id = session_id;
        }
        self.resize_session(session_id, size);
        Ok(String::new())
    }

    fn refresh_client(&self, target: Option<&str>) -> CommandResult {
        if self.resolve_client_ids(target).is_empty() {
            return Err("no matching client".to_owned());
        }
        Ok(String::new())
    }

    fn run_shell(
        &mut self,
        command: &str,
        background: bool,
        target: Option<&str>,
    ) -> CommandResult {
        if let Some(target) = target {
            let _ = self.resolve_pane_target(Some(target))?;
        }
        if background {
            Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .envs(self.environment.iter())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("run-shell: {error}"))?;
            return Ok(String::new());
        }
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .envs(self.environment.iter())
            .output()
            .map_err(|error| format!("run-shell: {error}"))?;
        if target.is_some() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn session_group_info(&self, session_index: usize) -> (bool, usize, String) {
        let ids = self.sessions[session_index]
            .windows
            .iter()
            .map(|window| window.id)
            .collect::<HashSet<_>>();
        let mut members = self
            .sessions
            .iter()
            .filter(|session| {
                session
                    .windows
                    .iter()
                    .any(|window| ids.contains(&window.id))
            })
            .map(|session| session.name.clone())
            .collect::<Vec<_>>();
        members.sort();
        let grouped = members.len() > 1;
        (grouped, members.len(), members.join(","))
    }

    fn expand_context_loops(
        &self,
        format: &str,
        session_index: usize,
        window_index: usize,
        pane_id: u64,
    ) -> String {
        let mut output = String::new();
        let mut index = 0;
        while index < format.len() {
            if format[index..].starts_with("##")
                || format[index..].starts_with("#,")
                || format[index..].starts_with("#:")
                || format[index..].starts_with("#}")
            {
                output.push_str(&format[index..index + 2]);
                index += 2;
                continue;
            }
            if !format[index..].starts_with("#{") {
                let character = format[index..]
                    .chars()
                    .next()
                    .expect("format index is on a character boundary");
                output.push(character);
                index += character.len_utf8();
                continue;
            }
            let Some(end) = format_token_end(format, index + 2) else {
                output.push_str(&format[index..]);
                break;
            };
            let body = &format[index + 2..end];
            if let Some((kind, spec, loop_format)) = parse_format_loop(body) {
                let mut items =
                    self.format_loop_items(kind, &spec, session_index, window_index, pane_id);
                if spec.contains('n') {
                    items.sort_by(|left, right| left.0.cmp(&right.0));
                }
                if spec.contains('z') {
                    items.sort_by(|left, right| right.0.cmp(&left.0));
                }
                if spec.contains('r') {
                    items.reverse();
                }
                for (_, item_session, item_window, item_pane, values) in items.drain(..) {
                    let nested = self.expand_context_loops(
                        &loop_format,
                        item_session,
                        item_window,
                        item_pane,
                    );
                    output.push_str(&render_format_with_options(
                        &nested,
                        &values,
                        &self.global_options,
                    ));
                }
            } else {
                output.push_str("#{");
                output.push_str(&self.expand_context_loops(
                    body,
                    session_index,
                    window_index,
                    pane_id,
                ));
                output.push('}');
            }
            index = end + 1;
        }
        output
    }

    fn format_loop_items(
        &self,
        kind: char,
        _spec: &str,
        session_index: usize,
        window_index: usize,
        pane_id: u64,
    ) -> Vec<(String, usize, usize, u64, Vec<(&'static str, String)>)> {
        match kind {
            'S' => self
                .sessions
                .iter()
                .enumerate()
                .map(|(index, session)| {
                    let values = vec![
                        ("session_id", format!("${}", session.id)),
                        ("session_name", session.name.clone()),
                        ("session_windows", session.windows.len().to_string()),
                    ];
                    (session.name.clone(), index, 0, 0, values)
                })
                .collect(),
            'W' => self
                .sessions
                .get(session_index)
                .map(|session| {
                    session
                        .windows
                        .iter()
                        .enumerate()
                        .map(|(index, window)| {
                            let values = vec![
                                ("session_name", session.name.clone()),
                                ("window_index", window.index.to_string()),
                                ("window_id", format!("@{}", window.id)),
                                ("window_name", window.name.clone()),
                                ("window_panes", window.panes.len().to_string()),
                            ];
                            (
                                window.name.clone(),
                                session_index,
                                index,
                                window.active_pane,
                                values,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            'P' => self
                .sessions
                .get(session_index)
                .and_then(|session| {
                    session
                        .windows
                        .get(window_index)
                        .map(|window| (session, window))
                })
                .map(|(session, window)| {
                    window
                        .panes
                        .iter()
                        .map(|pane| {
                            let values = vec![
                                ("session_name", session.name.clone()),
                                ("window_index", window.index.to_string()),
                                ("window_name", window.name.clone()),
                                ("pane_id", format!("%{}", pane.id)),
                                ("pane_index", pane.index.to_string()),
                                (
                                    "pane_active",
                                    if pane.id == window.active_pane {
                                        "1".to_owned()
                                    } else {
                                        "0".to_owned()
                                    },
                                ),
                            ];
                            (
                                pane.id.to_string(),
                                session_index,
                                window_index,
                                pane.id,
                                values,
                            )
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    let _ = pane_id;
                    Vec::new()
                }),
            _ => Vec::new(),
        }
    }

    fn group_member_indices(&self, session_index: usize) -> Vec<usize> {
        let ids = self.sessions[session_index]
            .windows
            .iter()
            .map(|window| window.id)
            .collect::<HashSet<_>>();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                session
                    .windows
                    .iter()
                    .any(|window| ids.contains(&window.id))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn sync_group_windows(&mut self, source_index: usize) {
        let members = self.group_member_indices(source_index);
        if members.len() < 2 {
            return;
        }
        let templates = self.sessions[source_index]
            .windows
            .iter()
            .map(Window::linked_clone)
            .collect::<Vec<_>>();
        for member in members {
            if member == source_index {
                continue;
            }
            let active = self.sessions[member].active_window;
            let last = self.sessions[member].last_window;
            self.sessions[member].windows = templates.iter().map(Window::linked_clone).collect();
            self.sessions[member].active_window =
                if templates.iter().any(|window| window.index == active) {
                    active
                } else {
                    self.sessions[member]
                        .windows
                        .first()
                        .map_or(self.sessions[member].base_index, |window| window.index)
                };
            self.sessions[member].last_window = last.filter(|index| {
                self.sessions[member]
                    .windows
                    .iter()
                    .any(|window| window.index == *index)
            });
            self.sessions[member].next_window_index = self.sessions[member]
                .windows
                .iter()
                .map(|window| window.index.saturating_add(1))
                .max()
                .unwrap_or(self.sessions[member].base_index);
            self.reflow_session(self.sessions[member].id);
        }
    }

    fn list_windows(&self, target: Option<&str>, format: Option<&str>) -> CommandResult {
        let session_index = self.resolve_session_index(target)?;
        let session = &self.sessions[session_index];
        let mut windows = session.windows.iter().collect::<Vec<_>>();
        windows.sort_by_key(|window| window.index);
        Ok(windows
            .into_iter()
            .enumerate()
            .map(|(window_position, window)| {
                let marker = if window.index == session.active_window {
                    '*'
                } else {
                    '-'
                };
                match format {
                    Some(format) => {
                        let pane_id = window.active_pane;
                        let format = self.expand_context_loops(
                            format,
                            session_index,
                            window_position,
                            pane_id,
                        );
                        render_format_with_options(
                            &format,
                            &[
                                ("session_name", session.name.clone()),
                                ("session_id", format!("${}", session.id)),
                                ("window_index", window.index.to_string()),
                                ("window_id", format!("@{}", window.id)),
                                ("window_name", window.name.clone()),
                                ("window_panes", window.panes.len().to_string()),
                                (
                                    "window_linked",
                                    if self.window_link_count(window.id) > 1 {
                                        "1"
                                    } else {
                                        "0"
                                    }
                                    .to_owned(),
                                ),
                                (
                                    "window_linked_sessions",
                                    self.window_link_count(window.id).to_string(),
                                ),
                                (
                                    "window_bell_flag",
                                    if window.bell_alert { "1" } else { "0" }.to_owned(),
                                ),
                                ("window_flags", window_alert_flags(window)),
                                ("window_width", window.size.cols.to_string()),
                                ("window_height", window.size.rows.to_string()),
                                (
                                    "window_active",
                                    if marker == '*' { "1" } else { "0" }.to_owned(),
                                ),
                            ],
                            &self.global_options,
                        )
                    }
                    None => format!(
                        "{}: {} {} {}",
                        window.index,
                        window.name,
                        marker,
                        window.panes.len()
                    ),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn list_panes(&self, target: Option<&str>, format: Option<&str>) -> CommandResult {
        let (session_index, window_index, _) = self.resolve_pane_target(target)?;
        let session = &self.sessions[session_index];
        let window = &self.sessions[session_index].windows[window_index];
        let session_names = self
            .sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>()
            .join("\0");
        let window_names = session
            .windows
            .iter()
            .map(|window| window.name.as_str())
            .collect::<Vec<_>>()
            .join("\0");
        Ok(window
            .panes
            .iter()
            .map(|pane| {
                let marker = if pane.id == window.active_pane {
                    '*'
                } else {
                    '-'
                };
                match format {
                    Some(format) => {
                        let format =
                            self.expand_context_loops(format, session_index, window_index, pane.id);
                        format_pane(
                            session,
                            window,
                            pane,
                            self.marked_pane == Some(pane.id),
                            self.marked_pane.is_some(),
                            pane.copy_mode.as_ref(),
                            self.window_link_count(window.id),
                            self.session_group_info(session_index),
                            &session_names,
                            &window_names,
                            &self.global_options,
                            &format,
                        )
                    }
                    None => format!("%{}: {} {}", pane.id, pane.index, marker),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn next_window(&mut self, target: Option<&str>) -> CommandResult {
        let session_index = self.resolve_session_index(target)?;
        let selected_window = {
            let session = &mut self.sessions[session_index];
            if session.windows.len() > 1 {
                let position = session
                    .windows
                    .iter()
                    .position(|window| window.index == session.active_window)
                    .unwrap_or(0);
                let next = session.windows[(position + 1) % session.windows.len()].index;
                session.select_window(next);
                session
                    .windows
                    .iter()
                    .find(|window| window.index == next)
                    .map(|window| window.id)
            } else {
                None
            }
        };
        if let Some(window_id) = selected_window {
            self.clear_window_alert(window_id);
        }
        Ok(String::new())
    }

    fn previous_window(&mut self, target: Option<&str>) -> CommandResult {
        let session_index = self.resolve_session_index(target)?;
        let selected_window = {
            let session = &mut self.sessions[session_index];
            if session.windows.len() > 1 {
                let position = session
                    .windows
                    .iter()
                    .position(|window| window.index == session.active_window)
                    .unwrap_or(0);
                let previous = if position == 0 {
                    session.windows.len() - 1
                } else {
                    position - 1
                };
                let previous = session.windows[previous].index;
                session.select_window(previous);
                session
                    .windows
                    .iter()
                    .find(|window| window.index == previous)
                    .map(|window| window.id)
            } else {
                None
            }
        };
        if let Some(window_id) = selected_window {
            self.clear_window_alert(window_id);
        }
        Ok(String::new())
    }

    fn select_window(&mut self, target: &str) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(Some(target))?;
        let index = self.sessions[session_index].windows[window_index].index;
        let window_id = self.sessions[session_index].windows[window_index].id;
        self.sessions[session_index].select_window(index);
        self.clear_window_alert(window_id);
        Ok(String::new())
    }

    fn clear_window_alert(&mut self, window_id: u64) {
        for session in &mut self.sessions {
            for window in &mut session.windows {
                if window.id == window_id {
                    window.bell_alert = false;
                }
            }
        }
    }

    fn rotate_window(&mut self, target: Option<&str>, up: bool) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        let session_id = self.sessions[session_index].id;
        self.sessions[session_index].windows[window_index].rotate_panes(up);
        self.reflow_session(session_id);
        Ok(String::new())
    }

    fn swap_window(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        detached: bool,
    ) -> CommandResult {
        let (target_session, target_index) = self.resolve_window_target(target)?;
        let (source_session, source_index) = if let Some(source) = source {
            self.resolve_window_target(Some(source))?
        } else {
            let session = &self.sessions[target_session];
            let index = session
                .windows
                .iter()
                .position(|window| window.index == session.active_window)
                .ok_or_else(|| "window not found: active".to_owned())?;
            (target_session, index)
        };
        if source_session != target_session {
            return Err("swap-window requires windows in the same session".to_owned());
        }
        if source_index == target_index {
            return Ok(String::new());
        }
        let session = &mut self.sessions[source_session];
        let source_number = session.windows[source_index].index;
        let target_number = session.windows[target_index].index;
        session.windows.swap(source_index, target_index);
        session.windows[source_index].index = source_number;
        session.windows[target_index].index = target_number;
        // tmux keeps the current index selected for the normal form, so a
        // chained `select-window -t +/-1` can finish the user's move. `-d`
        // instead keeps the source window selected at its new index.
        if detached {
            session.select_window(target_number);
        }
        Ok(String::new())
    }

    fn link_window(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        detached: bool,
        force: bool,
    ) -> CommandResult {
        let source = source.ok_or_else(|| "link-window requires a source".to_owned())?;
        let (source_session, source_position) = self.resolve_window_target(Some(source))?;
        let destination_session = self.resolve_session_index(target)?;
        let target_part = target
            .and_then(|target| target.split_once(':').map(|(_, rest)| rest))
            .filter(|rest| !rest.is_empty());
        let destination_number = if let Some(part) = target_part {
            if let Ok(index) = part.parse::<u32>() {
                index
            } else {
                return Err("link-window target must be an index".to_owned());
            }
        } else {
            let session = &self.sessions[destination_session];
            (session.base_index..=session.next_window_index)
                .find(|index| session.windows.iter().all(|window| window.index != *index))
                .unwrap_or(session.next_window_index)
        };
        let source_window = self.sessions[source_session].windows[source_position].linked_clone();
        if source_session == destination_session && source_window.index == destination_number {
            return Ok(String::new());
        }
        if let Some(position) = self.sessions[destination_session]
            .windows
            .iter()
            .position(|window| window.index == destination_number)
        {
            if !force {
                return Err(format!("index in use: {destination_number}"));
            }
            let old = self.sessions[destination_session].windows.remove(position);
            if self.window_link_count(old.id) == 0 {
                for pane in old.panes {
                    pane.pty.kill();
                }
            }
        }
        let mut linked = source_window;
        linked.index = destination_number;
        self.sessions[destination_session].windows.push(linked);
        self.sessions[destination_session]
            .windows
            .sort_by_key(|window| window.index);
        self.sessions[destination_session].next_window_index = self.sessions[destination_session]
            .next_window_index
            .max(destination_number.saturating_add(1));
        if !detached {
            self.sessions[destination_session].select_window(destination_number);
        }
        let source_id = self.sessions[source_session].id;
        let destination_id = self.sessions[destination_session].id;
        self.reflow_session(source_id);
        if source_id != destination_id {
            self.reflow_session(destination_id);
        }
        Ok(String::new())
    }

    fn unlink_window(&mut self, target: Option<&str>, force: bool) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        let window_id = self.sessions[session_index].windows[window_index].id;
        let links = self.window_link_count(window_id);
        if links <= 1 && !force {
            return Err("window only linked to one session".to_owned());
        }
        let window = self.sessions[session_index].windows.remove(window_index);
        if links <= 1 {
            for pane in window.panes {
                pane.pty.kill();
            }
        }
        if self.sessions[session_index].windows.is_empty() {
            self.sessions.remove(session_index);
        } else {
            let session = &mut self.sessions[session_index];
            if session.active_window == window.index {
                let replacement = session.windows[0].index;
                session.select_window(replacement);
            }
            if session.renumber_windows {
                renumber_session_windows(session);
            }
            let session_id = session.id;
            self.reflow_session(session_id);
        }
        Ok(String::new())
    }

    fn window_link_count(&self, window_id: u64) -> usize {
        self.sessions
            .iter()
            .flat_map(|session| &session.windows)
            .filter(|window| window.id == window_id)
            .count()
    }

    fn move_window(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        after: bool,
        detached: bool,
        force: bool,
        renumber: bool,
    ) -> CommandResult {
        if renumber && source.is_none() {
            let session_index = self.resolve_session_index(target)?;
            let session_id = self.sessions[session_index].id;
            renumber_session_windows(&mut self.sessions[session_index]);
            self.reflow_session(session_id);
            return Ok(String::new());
        }
        let source_target = source.or(target);
        let (source_session, source_position) = self.resolve_window_target(source_target)?;
        let source_number = self.sessions[source_session].windows[source_position].index;
        let destination_session = self.resolve_session_index(target)?;
        let destination_number = if after {
            let (_, target_position) = self.resolve_window_target(target)?;
            self.sessions[destination_session].windows[target_position]
                .index
                .saturating_add(1)
        } else {
            let target_part = target
                .and_then(|target| target.split_once(':').map(|(_, rest)| rest))
                .filter(|rest| !rest.is_empty());
            if let Some(part) = target_part {
                if let Ok(index) = part.parse::<u32>() {
                    index
                } else {
                    self.sessions[destination_session]
                        .windows
                        .iter()
                        .find(|window| window.name == part)
                        .map(|window| window.index)
                        .ok_or_else(|| format!("window not found: {part}"))?
                }
            } else {
                (0..=self.sessions[destination_session].next_window_index)
                    .find(|index| {
                        self.sessions[destination_session]
                            .windows
                            .iter()
                            .all(|window| window.index != *index)
                    })
                    .unwrap_or(self.sessions[destination_session].next_window_index)
            }
        };
        let same_source =
            source_session == destination_session && source_number == destination_number && !after;
        if same_source {
            return Ok(String::new());
        }
        if !after
            && self.sessions[destination_session]
                .windows
                .iter()
                .any(|window| window.index == destination_number && window.index != source_number)
            && !force
        {
            return Err(format!("index in use: {destination_number}"));
        }

        let source_id = self.sessions[source_session].id;
        let destination_id = self.sessions[destination_session].id;
        let window = self.sessions[source_session]
            .windows
            .remove(source_position);
        if self.sessions[source_session].windows.is_empty() {
            self.sessions[source_session].select_window(source_number);
        } else if self.sessions[source_session].active_window == source_number {
            let index = self.sessions[source_session].windows[0].index;
            self.sessions[source_session].select_window(index);
        }
        if !after {
            if let Some(position) = self.sessions[destination_session]
                .windows
                .iter()
                .position(|window| window.index == destination_number)
            {
                let old = self.sessions[destination_session].windows.remove(position);
                if self.window_link_count(old.id) == 0 {
                    for pane in old.panes {
                        pane.pty.kill();
                    }
                }
            }
        } else {
            for existing in &mut self.sessions[destination_session].windows {
                if existing.index >= destination_number {
                    existing.index = existing.index.saturating_add(1);
                }
            }
        }
        let mut window = window;
        window.index = destination_number;
        self.sessions[destination_session].windows.push(window);
        self.sessions[destination_session]
            .windows
            .sort_by_key(|window| window.index);
        self.sessions[destination_session].next_window_index = self.sessions[destination_session]
            .next_window_index
            .max(destination_number.saturating_add(1));
        if !detached {
            self.sessions[destination_session].select_window(destination_number);
        }
        if renumber {
            for session_index in [source_session, destination_session]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
            {
                renumber_session_windows(&mut self.sessions[session_index]);
            }
        }
        if source_session < self.sessions.len() {
            self.sync_group_windows(source_session);
        }
        if destination_session < self.sessions.len() && destination_session != source_session {
            self.sync_group_windows(destination_session);
        }
        self.reflow_session(source_id);
        if destination_id != source_id {
            self.reflow_session(destination_id);
        }
        Ok(String::new())
    }

    fn rename_session(&mut self, target: Option<&str>, name: &str) -> CommandResult {
        let session_index = self.resolve_session_index(target)?;
        let name = render_format_with_options(
            name,
            &[
                ("pid", std::process::id().to_string()),
                ("session_name", self.sessions[session_index].name.clone()),
            ],
            &self.global_options,
        );
        if self
            .sessions
            .iter()
            .enumerate()
            .any(|(index, session)| index != session_index && session.name == name)
        {
            return Err(format!("duplicate session: {name}"));
        }
        self.sessions[session_index].name = name.to_owned();
        Ok(String::new())
    }

    fn rename_window(&mut self, target: Option<&str>, name: &str) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        let rendered_name = render_format_with_options(
            name,
            &[
                ("pid", std::process::id().to_string()),
                ("session_name", self.sessions[session_index].name.clone()),
                (
                    "window_name",
                    self.sessions[session_index].windows[window_index]
                        .name
                        .clone(),
                ),
            ],
            &self.global_options,
        );
        let window_id = self.sessions[session_index].windows[window_index].id;
        for window in self
            .sessions
            .iter_mut()
            .flat_map(|session| &mut session.windows)
            .filter(|window| window.id == window_id)
        {
            window.name = rendered_name.clone();
        }
        Ok(String::new())
    }

    fn select_pane(
        &mut self,
        target: Option<&str>,
        direction: PaneDirection,
        mark: Option<bool>,
        title: Option<String>,
        enabled: Option<bool>,
    ) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let window = &mut self.sessions[session_index].windows[window_index];
        if matches!(direction, PaneDirection::Last)
            && target.is_none()
            && mark.is_none()
            && title.is_none()
            && enabled.is_none()
            && window.last_pane.is_none()
        {
            return Err("no last pane".to_owned());
        }
        let current = window
            .pane(pane_id)
            .map(|pane| pane.rect)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        let next = match direction {
            PaneDirection::Next => next_pane_id(window, pane_id, 1),
            PaneDirection::Previous => next_pane_id(window, pane_id, -1),
            PaneDirection::Last
                if target.is_some() || mark.is_some() || title.is_some() || enabled.is_some() =>
            {
                Some(pane_id)
            }
            PaneDirection::Last => window.last_pane.or(Some(pane_id)),
            PaneDirection::Left
            | PaneDirection::Right
            | PaneDirection::Up
            | PaneDirection::Down => directional_pane(window, pane_id, current, direction),
        };
        if let Some(next) = next {
            if next != window.active_pane {
                window.last_pane = Some(window.active_pane);
            }
            window.active_pane = next;
        }
        let selected = window.active_pane;
        if let Some(mark) = mark {
            if mark {
                self.marked_pane = Some(selected);
            } else {
                self.marked_pane = None;
            }
        }
        if let Some(title) = title
            && let Some(pane) = window.panes.iter_mut().find(|pane| pane.id == selected)
        {
            pane.title = title;
        }
        if let Some(enabled) = enabled
            && let Some(pane) = window.panes.iter_mut().find(|pane| pane.id == selected)
        {
            pane.enabled = enabled;
        }
        Ok(String::new())
    }

    fn resize_pane(
        &mut self,
        target: Option<&str>,
        direction: PaneDirection,
        amount: i32,
        absolute: Option<u16>,
        absolute_percent: bool,
        zoom: bool,
    ) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let session_id = self.sessions[session_index].id;
        if zoom {
            let window = &mut self.sessions[session_index].windows[window_index];
            window.zoomed = !window.zoomed;
            self.reflow_session(session_id);
            self.sync_group_windows(session_index);
            return Ok(String::new());
        }
        let window_size = self.sessions[session_index].windows[window_index].size;
        let absolute = absolute.map(|value| {
            if !absolute_percent {
                return value;
            }
            let extent = match direction {
                PaneDirection::Left | PaneDirection::Right => window_size.cols,
                PaneDirection::Up | PaneDirection::Down => window_size.rows,
                _ => 0,
            };
            (u32::from(extent).saturating_mul(u32::from(value)) / 100) as u16
        });
        let axis = match direction {
            PaneDirection::Left | PaneDirection::Right => Axis::Horizontal,
            PaneDirection::Up | PaneDirection::Down => Axis::Vertical,
            _ => return Err("resize-pane requires a directional option".to_owned()),
        };
        let signed_amount = match direction {
            PaneDirection::Left | PaneDirection::Up => -amount.abs(),
            PaneDirection::Right | PaneDirection::Down => amount.abs(),
            _ => amount,
        };
        let window = &mut self.sessions[session_index].windows[window_index];
        if !window.layout.resize(
            Rect {
                x: 0,
                y: 0,
                cols: window_size.cols,
                rows: window_size.rows,
            },
            pane_id,
            axis,
            signed_amount,
            absolute,
        ) {
            return Err("unable to resize pane".to_owned());
        }
        self.reflow_session(session_id);
        self.sync_group_windows(session_index);
        Ok(String::new())
    }

    fn resize_mouse_separator(&mut self, session_id: u64, resize: MouseResize, first_size: u16) {
        let Some(session_index) = self
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        else {
            return;
        };
        let active_window = self.sessions[session_index].active_window;
        let Some(window) = self.sessions[session_index]
            .windows
            .iter_mut()
            .find(|window| window.index == active_window)
        else {
            return;
        };
        let size = window.size;
        let _ = window.layout.set_separator_size(
            Rect {
                x: 0,
                y: 0,
                cols: size.cols,
                rows: size.rows,
            },
            &resize.path,
            resize.axis,
            first_size,
        );
        self.reflow_session(session_id);
        self.sync_group_windows(session_index);
    }

    fn swap_pane(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        direction: Option<PaneDirection>,
        detached: bool,
    ) -> CommandResult {
        let (target_session, target_window, target_id) = self.resolve_pane_target(target)?;
        let source_id = if let Some(source) = source {
            let (source_session, source_window, source_id) =
                self.resolve_pane_target(Some(source))?;
            if source_session != target_session || source_window != target_window {
                return Err("source and target panes must be in the same window".to_owned());
            }
            source_id
        } else if let Some(direction) = direction {
            let window = &self.sessions[target_session].windows[target_window];
            let position = window
                .panes
                .iter()
                .position(|pane| pane.id == target_id)
                .ok_or_else(|| "target pane no longer exists".to_owned())?;
            match direction {
                PaneDirection::Previous => {
                    window.panes[(position + window.panes.len() - 1) % window.panes.len()].id
                }
                PaneDirection::Next => window.panes[(position + 1) % window.panes.len()].id,
                _ => return Ok(String::new()),
            }
        } else if let Some(marked) = self.marked_pane {
            marked
        } else {
            return Err("no marked pane".to_owned());
        };
        if source_id == target_id {
            return Err("source and target panes must be different".to_owned());
        }
        let session_id = self.sessions[target_session].id;
        let window = &mut self.sessions[target_session].windows[target_window];
        if !window.swap_panes(source_id, target_id) {
            return Err("unable to swap panes".to_owned());
        }
        if !detached {
            window.active_pane = target_id;
        }
        self.reflow_session(session_id);
        Ok(String::new())
    }

    fn break_pane(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        name: Option<&str>,
        detached: bool,
        format: Option<&str>,
    ) -> CommandResult {
        let source_target = source.or(target);
        let (source_session, source_window, source_id) = self.resolve_pane_target(source_target)?;
        let target_session = self.resolve_session_index(target)?;
        if source_session != target_session {
            return Err("source and target must be in the same session".to_owned());
        }
        let session_id = self.sessions[source_session].id;
        let pane_position = self.sessions[source_session].windows[source_window]
            .panes
            .iter()
            .position(|pane| pane.id == source_id)
            .ok_or_else(|| "source pane no longer exists".to_owned())?;
        let mut pane = self.sessions[source_session].windows[source_window]
            .panes
            .remove(pane_position);
        pane.index = 0;
        let source_empty = self.sessions[source_session].windows[source_window]
            .panes
            .is_empty();
        let _ = self.sessions[source_session].windows[source_window]
            .layout
            .remove(source_id);
        if source_empty {
            self.sessions[source_session].windows.remove(source_window);
            if self.sessions[source_session].windows.is_empty() {
                self.sessions.remove(source_session);
                return Ok(String::new());
            }
        }
        let new_index = self.sessions[target_session].next_window_index;
        self.sessions[target_session].next_window_index += 1;
        let window_size = self.sessions[target_session].size;
        let window_id = self.next_window_id;
        self.next_window_id += 1;
        let window = Window::new(
            window_id,
            new_index,
            name.unwrap_or("0").to_owned(),
            window_size,
            pane,
        );
        self.sessions[target_session].windows.push(window);
        if !detached {
            self.sessions[target_session].select_window(new_index);
        }
        self.reflow_session(session_id);
        if let Some(format) = format {
            let session = &self.sessions[target_session];
            let window = session
                .windows
                .iter()
                .find(|window| window.index == new_index)
                .ok_or_else(|| "new window no longer exists".to_owned())?;
            let pane = window
                .pane(source_id)
                .ok_or_else(|| "moved pane no longer exists".to_owned())?;
            Ok(render_format(
                format,
                &[
                    ("session_name", session.name.clone()),
                    ("window_index", new_index.to_string()),
                    ("window_name", window.name.clone()),
                    ("pane_id", format!("%{}", pane.id)),
                    ("pane_index", pane.index.to_string()),
                ],
            ))
        } else {
            Ok(format!("{new_index}"))
        }
    }

    fn join_pane(
        &mut self,
        source: Option<&str>,
        target: Option<&str>,
        horizontal: bool,
        before: bool,
        detached: bool,
        split_size: Option<&str>,
    ) -> CommandResult {
        let (source_session, source_window, source_id) = self.resolve_pane_target(source)?;
        let (target_session, mut target_window, target_id) = self.resolve_pane_target(target)?;
        if source_id == target_id {
            return Err("source and target panes must be different".to_owned());
        }
        if source_session != target_session || source_window == target_window {
            return Err("source and target panes must be in different windows".to_owned());
        }
        let session_id = self.sessions[target_session].id;
        let source_window_number = self.sessions[source_session].windows[source_window].index;
        let target_window_number = self.sessions[target_session].windows[target_window].index;
        let source_was_active = self.sessions[source_session].active_window == source_window_number;
        let pane_position = self.sessions[source_session].windows[source_window]
            .panes
            .iter()
            .position(|pane| pane.id == source_id)
            .ok_or_else(|| "source pane no longer exists".to_owned())?;
        let pane = self.sessions[source_session].windows[source_window]
            .panes
            .remove(pane_position);
        let source_empty = self.sessions[source_session].windows[source_window]
            .panes
            .is_empty();
        let _ = self.sessions[source_session].windows[source_window]
            .layout
            .remove(source_id);
        if source_empty {
            self.sessions[source_session].windows.remove(source_window);
            if source_window < target_window {
                target_window = target_window.saturating_sub(1);
            }
            if source_was_active {
                // The source window disappears when its last pane is joined.
                // Keep the session's active-window invariant true; tmux leaves
                // the destination window selected in this situation.
                self.sessions[target_session].active_window = target_window_number;
                self.sessions[target_session].last_window = None;
            }
        }
        let target_rect = self.sessions[target_session].windows[target_window]
            .pane(target_id)
            .map(|pane| pane.rect)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        let requested =
            split_size.and_then(|value| parse_split_size(value, horizontal, target_rect));
        let available = if horizontal {
            target_rect.cols.saturating_sub(1)
        } else {
            target_rect.rows.saturating_sub(1)
        };
        let first_size = requested.map(|requested| {
            if before {
                requested.min(available)
            } else {
                available.saturating_sub(requested)
            }
        });
        let pane_id = pane.id;
        let window = &mut self.sessions[target_session].windows[target_window];
        let pane_index = window.next_pane_index;
        if !window.layout.split_with_size(
            target_id,
            pane_id,
            if horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            before,
            false,
            first_size,
        ) {
            return Err("target pane is not in the active layout".to_owned());
        }
        let mut pane = pane;
        pane.index = pane_index;
        window.next_pane_index += 1;
        window.panes.push(pane);
        if !detached {
            window.active_pane = pane_id;
        }
        self.reflow_session(session_id);
        Ok(String::new())
    }

    fn respawn_pane(
        &mut self,
        shared: &SharedState,
        target: Option<&str>,
        command: &[String],
        cwd: Option<&str>,
        kill: bool,
        empty: bool,
        window_command: bool,
    ) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let (size, existing_dead, session_cwd, stored_command) = {
            let pane = self.sessions[session_index].windows[window_index]
                .pane(pane_id)
                .ok_or_else(|| "target pane no longer exists".to_owned())?;
            (
                pane.rect_size(),
                pane.dead,
                self.sessions[session_index].cwd.clone(),
                pane.command_args.clone(),
            )
        };
        if window_command {
            let window_active = self.sessions[session_index].windows[window_index]
                .panes
                .iter()
                .any(|pane| !pane.dead);
            if window_active && !kill {
                return Err("respawn window failed: window is still active".to_owned());
            }
        }
        if !existing_dead && !kill {
            return Err("respawn pane failed: pane is still active".to_owned());
        }
        let effective_command = if command.is_empty() {
            stored_command
        } else {
            command.to_vec()
        };
        let start_path = cwd
            .map(str::to_owned)
            .or_else(|| session_cwd.clone())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned())
            });
        if window_command {
            let session_id = self.sessions[session_index].id;
            let pane_id = self.next_pane_id();
            let pane = self.new_pane(
                shared,
                pane_id,
                0,
                size,
                &effective_command,
                cwd.or(session_cwd.as_deref()),
                empty,
            )?;
            let window = &mut self.sessions[session_index].windows[window_index];
            for old in window.panes.drain(..) {
                old.pty.kill();
            }
            window.layout = crate::model::Layout::Leaf(pane_id);
            window.panes.push(pane);
            window.active_pane = pane_id;
            window.last_pane = None;
            window.next_pane_index = 1;
            self.reflow_session(session_id);
            self.sync_group_windows(session_index);
            return Ok(String::new());
        }
        let pty = if empty {
            Pty::empty().map_err(|error| error.to_string())?
        } else {
            Pty::spawn(
                &effective_command,
                cwd.or(session_cwd.as_deref()).map(Path::new),
                size,
                self.global_options
                    .get("default-terminal")
                    .map(String::as_str),
                &self.environment,
            )
            .map_err(|error| error.to_string())?
        };
        let reader = if empty {
            None
        } else {
            Some(pty.reader().map_err(|error| error.to_string())?)
        };
        let pid = pty.pid();
        self.pane_pipes.remove(&pane_id);
        let pane = self.sessions[session_index].windows[window_index]
            .panes
            .iter_mut()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        if kill {
            pane.pty.kill();
        }
        pane.pty = pty;
        pane.parser = Parser::new(size.rows, size.cols, self.history_limit);
        pane.output_state = terminal::OutputState::default();
        pane.copy_mode = None;
        pane.copy_source = None;
        pane.raw_output.clear();
        pane.history_floor = 0;
        pane.dead = false;
        pane.command = if empty {
            String::new()
        } else {
            effective_command.join(" ")
        };
        pane.command_args = effective_command;
        pane.current_path = start_path.clone();
        pane.start_path = start_path;
        pane.dead = false;
        if let Some(reader) = reader {
            spawn_reader(Arc::clone(shared), pane_id, pid, reader);
        }
        self.sync_group_windows(session_index);
        Ok(String::new())
    }

    fn kill_pane(
        &mut self,
        target: Option<&str>,
        all: bool,
        filter: Option<&str>,
    ) -> CommandResult {
        let (session_index, window_index, pane_id) = self.resolve_pane_target(target)?;
        let session_id = self.sessions[session_index].id;
        let linked_window =
            self.window_link_count(self.sessions[session_index].windows[window_index].id) > 1;
        if all || filter.is_some() {
            let ids = self.sessions[session_index].windows[window_index]
                .panes
                .iter()
                .filter(|pane| pane.id != pane_id && pane_filter(pane, filter))
                .map(|pane| pane.id)
                .collect::<Vec<_>>();
            for id in &ids {
                self.pane_pipes.remove(id);
            }
            let window = &mut self.sessions[session_index].windows[window_index];
            for id in &ids {
                if let Some(position) = window.panes.iter().position(|pane| pane.id == *id) {
                    let pane = window.panes.remove(position);
                    if !linked_window {
                        pane.pty.kill();
                    }
                    let _ = window.layout.remove(*id);
                }
            }
            if window.panes.is_empty() {
                self.sessions[session_index].windows.remove(window_index);
            } else {
                window.active_pane = pane_id;
            }
            self.reflow_session(session_id);
            return Ok(String::new());
        }
        self.pane_pipes.remove(&pane_id);
        let window = &mut self.sessions[session_index].windows[window_index];
        let Some(position) = window.panes.iter().position(|pane| pane.id == pane_id) else {
            return Err("target pane no longer exists".to_owned());
        };
        let pane = window.panes.remove(position);
        if !linked_window {
            pane.pty.kill();
        }
        if window.panes.is_empty() {
            self.sessions[session_index].windows.remove(window_index);
            if self.sessions[session_index].windows.is_empty() {
                self.sessions.remove(session_index);
            } else {
                let new_index = self.sessions[session_index].windows[0].index;
                self.sessions[session_index].select_window(new_index);
                if self.sessions[session_index].renumber_windows {
                    renumber_session_windows(&mut self.sessions[session_index]);
                }
            }
            return Ok(String::new());
        }
        let _ = window.layout.remove(pane_id);
        if window.active_pane == pane_id {
            window.active_pane = window.panes[0].id;
        }
        self.reflow_session(session_id);
        Ok(String::new())
    }

    fn kill_window(&mut self, target: Option<&str>, all: bool) -> CommandResult {
        let (session_index, window_index) = self.resolve_window_target(target)?;
        let session_id = self.sessions[session_index].id;
        if all {
            let keep_index = self.sessions[session_index].windows[window_index].index;
            let windows = std::mem::take(&mut self.sessions[session_index].windows);
            let mut kept = Vec::with_capacity(1);
            for window in windows {
                if window.index == keep_index {
                    kept.push(window);
                } else {
                    for pane in &window.panes {
                        self.pane_pipes.remove(&pane.id);
                    }
                    if self.window_link_count(window.id) == 0 {
                        for pane in window.panes {
                            pane.pty.kill();
                        }
                    }
                }
            }
            self.sessions[session_index].windows = kept;
            self.sessions[session_index].select_window(keep_index);
            self.sync_group_windows(session_index);
            self.reflow_session(session_id);
            return Ok(String::new());
        }
        let window = self.sessions[session_index].windows.remove(window_index);
        for pane in &window.panes {
            self.pane_pipes.remove(&pane.id);
        }
        let last_link = self.window_link_count(window.id) == 0;
        if last_link {
            for pane in window.panes {
                pane.pty.kill();
            }
        }
        if self.sessions[session_index].windows.is_empty() {
            self.sessions.remove(session_index);
        } else if self.sessions[session_index].active_window == window.index {
            let index = self.sessions[session_index].windows[0].index;
            self.sessions[session_index].select_window(index);
            if self.sessions[session_index].renumber_windows {
                renumber_session_windows(&mut self.sessions[session_index]);
            }
        } else {
            if self.sessions[session_index].renumber_windows {
                renumber_session_windows(&mut self.sessions[session_index]);
            }
            self.reflow_session(session_id);
        }
        if session_index < self.sessions.len() {
            self.sync_group_windows(session_index);
        }
        Ok(String::new())
    }

    fn capture_pane(
        &mut self,
        target: Option<&str>,
        start: Option<i32>,
        end: Option<i32>,
        escape: bool,
        join: bool,
        preserve_trailing: bool,
    ) -> CommandResult {
        let (_, _, pane_id) = self.resolve_pane_target(target)?;
        let pane = self
            .find_pane_mut(pane_id)
            .ok_or_else(|| "target pane no longer exists".to_owned())?;
        let displayed_scrollback = pane.parser.screen().scrollback();
        let (mut history, live) = history_rows(&mut pane.parser);
        let floor = pane.history_floor.min(history.len());
        history.drain(..floor);
        let base = history.len();
        let live_rows = live.len();
        let total_rows = base.saturating_add(live_rows);
        let default_view = start.is_none() && end.is_none();
        let first = start.map_or_else(
            || {
                if default_view {
                    base.saturating_sub(displayed_scrollback)
                } else {
                    base
                }
            },
            |offset| {
                if offset == i32::MIN {
                    0
                } else {
                    capture_absolute_offset(offset, base)
                }
            },
        );
        let last = end.map_or_else(
            || {
                if default_view {
                    base.saturating_sub(displayed_scrollback)
                        .saturating_add(live_rows.saturating_sub(1))
                } else {
                    base.saturating_add(live_rows.saturating_sub(1))
                }
            },
            |offset| {
                if offset == i32::MIN {
                    total_rows.saturating_sub(1)
                } else {
                    capture_absolute_offset(offset, base)
                }
            },
        );
        let first = first.min(base.saturating_add(live_rows));
        let last = last.min(base.saturating_add(live_rows).saturating_sub(1));
        let count = last.saturating_sub(first).saturating_add(1);
        let mut output = Vec::new();
        if escape {
            for index in 0..count {
                let row =
                    capture_formatted_row(&mut pane.parser, base, first + index, &pane.raw_output);
                output.extend_from_slice(&row);
                if !join && index + 1 < count {
                    output.push(b'\n');
                }
            }
        } else {
            let lines = history.into_iter().chain(live).collect::<Vec<_>>();
            for index in 0..count {
                let line = &lines[first + index];
                output.extend_from_slice(line.as_bytes());
                if preserve_trailing {
                    let padding =
                        usize::from(pane.rect.cols).saturating_sub(format_display_width(line));
                    output.extend(std::iter::repeat_n(b' ', padding));
                }
                if !join && index + 1 < count {
                    output.push(b'\n');
                }
            }
        }
        Ok(String::from_utf8_lossy(&output)
            .trim_end_matches('\n')
            .to_owned())
    }

    fn kill_session(&mut self, target: Option<&str>, all: bool) -> CommandResult {
        let index = self.resolve_session_index(target)?;
        if all {
            let keep_id = self.sessions[index].id;
            let sessions = std::mem::take(&mut self.sessions);
            let kept_window_ids = sessions
                .iter()
                .find(|session| session.id == keep_id)
                .map(|session| {
                    session
                        .windows
                        .iter()
                        .map(|window| window.id)
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            let mut kept = Vec::with_capacity(1);
            for session in sessions {
                if session.id == keep_id {
                    kept.push(session);
                } else {
                    for window in session.windows {
                        for pane in &window.panes {
                            self.pane_pipes.remove(&pane.id);
                        }
                        if !kept_window_ids.contains(&window.id) {
                            for pane in window.panes {
                                pane.pty.kill();
                            }
                        }
                    }
                }
            }
            self.sessions = kept;
            return Ok(String::new());
        }
        let session = self.sessions.remove(index);
        for window in session.windows {
            for pane in &window.panes {
                self.pane_pipes.remove(&pane.id);
            }
            if self.window_link_count(window.id) == 0 {
                for pane in window.panes {
                    pane.pty.kill();
                }
            }
        }
        Ok(String::new())
    }

    fn render_session(&mut self, session_id: u64, client_id: Option<u64>) -> Option<Vec<u8>> {
        self.render_session_with_clear(session_id, client_id, true)
    }

    fn render_session_with_clear(
        &mut self,
        session_id: u64,
        client_id: Option<u64>,
        clear_screen: bool,
    ) -> Option<Vec<u8>> {
        let client_prefix = client_id
            .and_then(|client_id| self.clients.get(&client_id))
            .is_some_and(|client| client.prefix_pending);
        let tree_mode = client_id
            .and_then(|client_id| self.clients.get(&client_id))
            .and_then(|client| client.tree_mode.clone());
        let buffer_mode = client_id
            .and_then(|client_id| self.clients.get(&client_id))
            .and_then(|client| client.buffer_mode.clone());
        let client_mode = client_id
            .and_then(|client_id| self.clients.get(&client_id))
            .and_then(|client| client.client_mode.clone());
        let panes_mode = client_id
            .and_then(|client_id| self.clients.get(&client_id))
            .and_then(|client| client.panes_mode.clone());
        let (copy_prompt_message, copy_prompt_cursor) = client_id
            .and_then(|client_id| {
                let session_id = self.clients.get(&client_id)?.session_id;
                self.sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(|session| session.active_window())
                    .and_then(|window| window.active())
                    .and_then(|pane| pane.copy_mode.as_ref())
                    .map(|mode| (mode.prompt_display(), mode.prompt_cursor_display()))
            })
            .unwrap_or((None, None));
        let pane_prompt_message = client_id.and_then(|client_id| {
            self.clients
                .get(&client_id)
                .and_then(|client| client.prompt.as_ref())
                .filter(|prompt| prompt.pane)
                .map(|prompt| format!("{}{}", prompt.label, display_prompt_input(&prompt.input)))
        });
        let pane_prompt_cursor = client_id.and_then(|client_id| {
            self.clients
                .get(&client_id)
                .and_then(|client| client.prompt.as_ref())
                .filter(|prompt| prompt.pane)
                .map(AttachedPrompt::cursor_display)
        });
        let session = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)?;
        let mut status_output = Vec::new();
        let message = self.last_message.take();
        let prompt_message = client_id
            .and_then(|client_id| {
                self.clients.get(&client_id).and_then(|client| {
                    client
                        .prompt
                        .as_ref()
                        .filter(|prompt| !prompt.pane)
                        .map(|prompt| {
                            format!("{}{}", prompt.label, display_prompt_input(&prompt.input))
                        })
                })
            })
            .or(copy_prompt_message);
        let status_prompt_cursor = if message.is_none()
            && !self
                .global_options
                .get("status")
                .is_some_and(|value| !parse_on_off(value).unwrap_or(true))
        {
            client_id
                .and_then(|client_id| {
                    self.clients.get(&client_id).and_then(|client| {
                        client
                            .prompt
                            .as_ref()
                            .filter(|prompt| !prompt.pane)
                            .map(AttachedPrompt::cursor_display)
                    })
                })
                .or(copy_prompt_cursor)
        } else {
            None
        };
        if let Some(status_window) = session
            .windows
            .iter()
            .find(|window| window.index == session.active_window)
        {
            render_status_line(
                &mut status_output,
                session,
                status_window,
                client_prefix,
                &self.global_options,
                message.as_deref(),
                prompt_message.as_deref(),
            );
        }
        let window = session
            .windows
            .iter_mut()
            .find(|window| window.index == session.active_window)?;
        let history_limit = self.history_limit;
        let mut output = Vec::new();
        let clipboard = self.clipboard_pending.take();
        let consumed_one_shot_state = message.is_some() || clipboard.is_some();
        if clear_screen {
            output.extend_from_slice(b"\x1b[?25l\x1b[2J\x1b[H\x1b[0m");
        } else {
            output.extend_from_slice(b"\x1b[?25l\x1b[H\x1b[0m");
        }
        if let Some(data) = clipboard {
            output.extend_from_slice(b"\x1b]52;c;");
            output.extend_from_slice(base64_encode(&data).as_bytes());
            output.push(0x07);
        }
        if self
            .global_options
            .get("mouse")
            .is_some_and(|value| parse_on_off(value).unwrap_or(false))
        {
            // Button-event tracking is required for terminal drag updates;
            // 1000 alone reports presses/releases but not motion while held.
            output.extend_from_slice(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h");
        }
        if self
            .global_options
            .get("focus-events")
            .is_some_and(|value| parse_on_off(value).unwrap_or(false))
        {
            output.extend_from_slice(b"\x1b[?1004h");
        }
        if self
            .global_options
            .get("extended-keys")
            .is_some_and(|value| value != "off")
        {
            output.extend_from_slice(b"\x1b[>1u");
        }
        for pane in &mut window.panes {
            let content_rect =
                pane_content_rect(pane, &window.options, &self.global_options, history_limit);
            let mut source_parser = copy_source_parser(pane, history_limit);
            let copy_cursor = pane.copy_mode.as_mut().map(|mode| {
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut pane.parser, |parser| parser);
                let (row, col) = mode.cursor_viewport(parser);
                let viewport_start = mode.viewport_start(parser);
                (row, col, mode.hide_position, viewport_start)
            });
            if copy_cursor.is_some() {
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut pane.parser, |parser| parser);
                let screen = parser.screen().clone();
                let lines = screen.rows(0, content_rect.cols).collect::<Vec<_>>();
                let mut cell_style = CellStyle::default();
                for row in 0..content_rect.rows {
                    output.extend_from_slice(
                        format!("\x1b[{};{}H", content_rect.y + row + 1, content_rect.x + 1)
                            .as_bytes(),
                    );
                    let line = lines
                        .get(usize::from(row))
                        .map(String::as_str)
                        .unwrap_or("");
                    let (cursor_row, cursor_col, hide_position, viewport_start) =
                        copy_cursor.expect("copy cursor exists for copy-mode rendering");
                    let logical_row = viewport_start + usize::from(row);
                    let line_number_width = pane
                        .copy_mode
                        .as_mut()
                        .filter(|mode| mode.line_numbers)
                        .map_or(0, |mode| {
                            let parser = source_parser
                                .as_mut()
                                .map_or(&mut pane.parser, |parser| parser);
                            copy_mode_line_number_width_from_mode(mode, parser)
                        });
                    if line_number_width > 0 {
                        output.extend_from_slice(b"\x1b[2m");
                        output.extend_from_slice(
                            format!(
                                "{:>width$} ",
                                pane.copy_mode.as_ref().map_or_else(
                                    || logical_row.saturating_add(1),
                                    |mode| { mode.line_number_value(logical_row) }
                                ),
                                width = line_number_width.saturating_sub(1)
                            )
                            .as_bytes(),
                        );
                        output.extend_from_slice(b"\x1b[0m");
                    }
                    let content_cols =
                        usize::from(content_rect.cols).saturating_sub(line_number_width);
                    let mut display_col = 0usize;
                    for (char_col, character) in line.chars().enumerate() {
                        if display_col >= content_cols {
                            break;
                        }
                        let style = screen
                            .cell(row, u16::try_from(char_col).unwrap_or(u16::MAX))
                            .map(CellStyle::from_cell)
                            .unwrap_or_default();
                        append_cell_style(&mut output, &mut cell_style, style);
                        let selected = pane
                            .copy_mode
                            .as_ref()
                            .is_some_and(|mode| mode.cell_selected(logical_row, char_col));
                        let cursor = !hide_position
                            && cursor_row == usize::from(row)
                            && cursor_col == display_col;
                        if selected || cursor {
                            output.extend_from_slice(b"\x1b[7m");
                        }
                        let mut encoded = [0u8; 4];
                        output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                        if selected || cursor {
                            output.extend_from_slice(b"\x1b[27m");
                            if style.inverse {
                                output.extend_from_slice(b"\x1b[7m");
                            }
                        }
                        display_col += if is_wide_copy_character(character) {
                            2
                        } else {
                            1
                        };
                    }
                    while display_col < content_cols {
                        let style = screen
                            .cell(row, u16::try_from(display_col).unwrap_or(u16::MAX))
                            .map(CellStyle::from_cell)
                            .unwrap_or_default();
                        append_cell_style(&mut output, &mut cell_style, style);
                        let selected = pane
                            .copy_mode
                            .as_ref()
                            .is_some_and(|mode| mode.cell_selected(logical_row, display_col));
                        let cursor = !hide_position
                            && cursor_row == usize::from(row)
                            && cursor_col == display_col;
                        if selected || cursor {
                            output.extend_from_slice(b"\x1b[7m");
                        }
                        output.push(b' ');
                        if selected || cursor {
                            output.extend_from_slice(b"\x1b[27m");
                            if style.inverse {
                                output.extend_from_slice(b"\x1b[7m");
                            }
                        }
                        display_col += 1;
                    }
                    output.extend_from_slice(b"\x1b[0m\x1b[K");
                    cell_style = CellStyle::default();
                }
                let position_format = window
                    .options
                    .get("copy-mode-position-format")
                    .or_else(|| self.global_options.get("copy-mode-position-format"))
                    .map(String::as_str)
                    .unwrap_or("#[align=right][#{copy_position}/#{copy_position_limit}]");
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut pane.parser, |parser| parser);
                if let Some(mode) = pane.copy_mode.as_mut() {
                    render_copy_mode_position(
                        &mut output,
                        content_rect,
                        mode,
                        parser,
                        position_format,
                        &self.global_options,
                    );
                }
            } else {
                let screen = pane.parser.screen();
                let mut cell_style = CellStyle::default();
                for row in 0..content_rect.rows {
                    output.extend_from_slice(
                        format!("\x1b[{};{}H", content_rect.y + row + 1, content_rect.x + 1)
                            .as_bytes(),
                    );
                    for col in 0..content_rect.cols {
                        if let Some(cell) = screen.cell(row, col) {
                            if cell.is_wide_continuation() {
                                continue;
                            }
                            let style = CellStyle::from_cell(cell);
                            append_cell_style(&mut output, &mut cell_style, style);
                            let contents = cell.contents();
                            if contents.is_empty() {
                                output.push(b' ');
                            } else {
                                output.extend_from_slice(contents.as_bytes());
                            }
                        } else {
                            output.push(b' ');
                        }
                    }
                    output.extend_from_slice(b"\x1b[0m\x1b[K");
                    cell_style = CellStyle::default();
                }
            }
            render_pane_scrollbar(
                &mut output,
                pane,
                &window.options,
                &self.global_options,
                history_limit,
            );
        }
        render_layout_separators(&mut output, window);
        if let Some(message) = pane_prompt_message
            && let Some(active) = window
                .panes
                .iter()
                .find(|pane| pane.id == window.active_pane)
        {
            output.extend_from_slice(
                format!(
                    "\x1b[{};{}H\x1b[K{}",
                    active.rect.y
                        + if !self
                            .global_options
                            .get("status")
                            .is_some_and(|value| { !parse_on_off(value).unwrap_or(true) })
                        {
                            active.rect.rows.saturating_sub(1)
                        } else {
                            active.rect.rows
                        },
                    active.rect.x + 1,
                    message
                )
                .as_bytes(),
            );
        }
        // The status bar is drawn with ordinary cursor-addressed output. Draw
        // it before restoring the active pane cursor so the terminal's
        // hardware cursor remains where the pane application owns it rather
        // than stranded on the status row.
        output.extend_from_slice(&status_output);
        if let Some(prompt) = status_prompt_cursor.as_deref() {
            render_status_prompt_cursor(&mut output, session.size, &self.global_options, prompt);
        } else if let Some(prompt) = pane_prompt_cursor.as_deref()
            && let Some(active) = window
                .panes
                .iter()
                .find(|pane| pane.id == window.active_pane)
        {
            render_pane_prompt_cursor(
                &mut output,
                active.rect,
                &self.global_options,
                prompt,
            );
        } else if let Some(active) = window
            .panes
            .iter_mut()
            .find(|pane| pane.id == window.active_pane)
        {
            let content_rect =
                pane_content_rect(active, &window.options, &self.global_options, history_limit);
            let mut source_parser = copy_source_parser(active, history_limit);
            if let Some(mode) = active.copy_mode.as_mut() {
                let parser = source_parser
                    .as_mut()
                    .map_or(&mut active.parser, |parser| parser);
                let (row, col) = mode.cursor_viewport(parser);
                let line_number_width = if mode.line_numbers {
                    let parser = source_parser
                        .as_mut()
                        .map_or(&mut active.parser, |parser| parser);
                    copy_mode_line_number_width_from_mode(mode, parser)
                } else {
                    0
                };
                if !mode.hide_position && !active.dead {
                    output.extend_from_slice(
                        format!(
                            "\x1b[{};{}H\x1b[?25h",
                            content_rect.y + u16::try_from(row).unwrap_or(u16::MAX) + 1,
                            content_rect.x
                                + u16::try_from(col.saturating_add(line_number_width))
                                    .unwrap_or(u16::MAX)
                                + 1
                        )
                        .as_bytes(),
                    );
                }
            } else if !active.parser.screen().hide_cursor() && !active.dead {
                let (row, col) = active.parser.screen().cursor_position();
                output.extend_from_slice(
                    format!(
                        "\x1b[{};{}H\x1b[?25h",
                        content_rect.y + row.min(content_rect.rows.saturating_sub(1)) + 1,
                        content_rect.x + col.min(content_rect.cols.saturating_sub(1)) + 1
                    )
                    .as_bytes(),
                );
            }
        }
        if let Some(tree_mode) = tree_mode.as_ref() {
            render_tree_mode_overlay(&mut output, tree_mode, session.size);
        } else if let Some(buffer_mode) = buffer_mode.as_ref() {
            render_buffer_mode_overlay(&mut output, buffer_mode, session.size);
        } else if let Some(client_mode) = client_mode.as_ref() {
            render_client_mode_overlay(&mut output, client_mode, session.size);
        } else if let Some(panes_mode) = panes_mode.as_ref() {
            render_panes_mode_overlay(&mut output, panes_mode, session.size);
        }
        if consumed_one_shot_state {
            self.mark_render_dirty();
        }
        Some(output)
    }

    fn resolve_session_index(&self, target: Option<&str>) -> Result<usize, String> {
        if self.sessions.is_empty() {
            return Err("no sessions".to_owned());
        }
        let target = target.unwrap_or("");
        let session_name = target.split(':').next().unwrap_or(target);
        if let Some(id) = session_name
            .strip_prefix('$')
            .and_then(|value| value.parse::<u64>().ok())
        {
            return self
                .sessions
                .iter()
                .position(|session| session.id == id)
                .ok_or_else(|| format!("session not found: ${id}"));
        }
        if let Some(id) = session_name
            .strip_prefix('@')
            .and_then(|value| value.parse::<u64>().ok())
        {
            return self
                .sessions
                .iter()
                .position(|session| session.windows.iter().any(|window| window.id == id))
                .ok_or_else(|| format!("window not found: @{id}"));
        }
        if session_name.is_empty() || (!target.contains(':') && self.find_session(target).is_none())
        {
            return Ok(0);
        }
        let exact = session_name.strip_prefix('=').unwrap_or(session_name);
        if session_name.starts_with('=') {
            return self
                .sessions
                .iter()
                .position(|session| session.name == exact)
                .ok_or_else(|| format!("session not found: {exact}"));
        }
        self.find_session(exact)
            .ok_or_else(|| format!("session not found: {exact}"))
    }

    fn resolve_window_target(&self, target: Option<&str>) -> Result<(usize, usize), String> {
        let target = target.unwrap_or("");
        if let Some(id) = target
            .strip_prefix('@')
            .and_then(|value| value.parse::<u64>().ok())
        {
            for (session_index, session) in self.sessions.iter().enumerate() {
                if let Some(window_index) =
                    session.windows.iter().position(|window| window.id == id)
                {
                    return Ok((session_index, window_index));
                }
            }
            return Err(format!("window not found: @{id}"));
        }
        let session_index = self.resolve_session_index(Some(target))?;
        let window_part = if let Some((_, rest)) = target.split_once(':') {
            rest
        } else if target.is_empty() || self.find_session(target).is_some() {
            ""
        } else {
            target
        };
        self.window_index(session_index, window_part)
    }

    fn resolve_pane_target(&self, target: Option<&str>) -> Result<(usize, usize, u64), String> {
        let target = target.unwrap_or("");
        if let Some(id) = target
            .strip_prefix('%')
            .and_then(|id| id.parse::<u64>().ok())
        {
            return self
                .find_pane_location(id)
                .ok_or_else(|| format!("pane not found: %{id}"));
        }
        if matches!(target, "~" | "{marked}") {
            let id = self
                .marked_pane
                .ok_or_else(|| "no marked target".to_owned())?;
            return self
                .find_pane_location(id)
                .ok_or_else(|| "marked pane no longer exists".to_owned());
        }
        let (window_target, pane_token) = target
            .rsplit_once('.')
            .map_or((target, None), |(window, pane)| (window, Some(pane)));
        let (session_index, window_index) = self.resolve_window_target(Some(window_target))?;
        let window = &self.sessions[session_index].windows[window_index];
        let token = pane_token.unwrap_or("");
        let pane_id = if token.is_empty() {
            window.active_pane
        } else if let Ok(index) = token.parse::<u32>() {
            window
                .pane_for_index(index)
                .ok_or_else(|| format!("pane not found: {token}"))?
        } else if token == "+" {
            next_pane_id(window, window.active_pane, 1)
                .ok_or_else(|| format!("pane not found: {token}"))?
        } else if token == "-" {
            next_pane_id(window, window.active_pane, -1)
                .ok_or_else(|| format!("pane not found: {token}"))?
        } else if token == "!" {
            window
                .last_pane
                .ok_or_else(|| format!("pane not found: {token}"))?
        } else if matches!(
            token,
            "{top-left}"
                | "{top-right}"
                | "{bottom-left}"
                | "{bottom-right}"
                | "{top}"
                | "{bottom}"
                | "{left}"
                | "{right}"
                | "{up-of}"
                | "{down-of}"
                | "{left-of}"
                | "{right-of}"
        ) {
            let selected = match token {
                "{up-of}" => directional_pane(
                    window,
                    window.active_pane,
                    window
                        .pane(window.active_pane)
                        .map(|pane| pane.rect)
                        .unwrap_or_default(),
                    PaneDirection::Up,
                ),
                "{down-of}" => directional_pane(
                    window,
                    window.active_pane,
                    window
                        .pane(window.active_pane)
                        .map(|pane| pane.rect)
                        .unwrap_or_default(),
                    PaneDirection::Down,
                ),
                "{left-of}" => directional_pane(
                    window,
                    window.active_pane,
                    window
                        .pane(window.active_pane)
                        .map(|pane| pane.rect)
                        .unwrap_or_default(),
                    PaneDirection::Left,
                ),
                "{right-of}" => directional_pane(
                    window,
                    window.active_pane,
                    window
                        .pane(window.active_pane)
                        .map(|pane| pane.rect)
                        .unwrap_or_default(),
                    PaneDirection::Right,
                ),
                "{top-left}" => window
                    .panes
                    .iter()
                    .min_by_key(|pane| (pane.rect.y, pane.rect.x))
                    .map(|pane| pane.id),
                "{top-right}" => window
                    .panes
                    .iter()
                    .min_by_key(|pane| (pane.rect.y, std::cmp::Reverse(pane.rect.x)))
                    .map(|pane| pane.id),
                "{bottom-left}" => window
                    .panes
                    .iter()
                    .max_by_key(|pane| (pane.rect.y, std::cmp::Reverse(pane.rect.x)))
                    .map(|pane| pane.id),
                "{bottom-right}" => window
                    .panes
                    .iter()
                    .max_by_key(|pane| (pane.rect.y, pane.rect.x))
                    .map(|pane| pane.id),
                "{top}" => window
                    .panes
                    .iter()
                    .min_by_key(|pane| (pane.rect.y, pane.rect.x))
                    .map(|pane| pane.id),
                "{bottom}" => window
                    .panes
                    .iter()
                    .max_by_key(|pane| (pane.rect.y, std::cmp::Reverse(pane.rect.x)))
                    .map(|pane| pane.id),
                "{left}" => window
                    .panes
                    .iter()
                    .min_by_key(|pane| (pane.rect.x, pane.rect.y))
                    .map(|pane| pane.id),
                "{right}" => window
                    .panes
                    .iter()
                    .min_by_key(|pane| (std::cmp::Reverse(pane.rect.x), pane.rect.y))
                    .map(|pane| pane.id),
                _ => None,
            };
            selected.ok_or_else(|| format!("pane not found: {token}"))?
        } else {
            return Err(format!("pane not found: {token}"));
        };
        Ok((session_index, window_index, pane_id))
    }

    fn window_index(&self, session_index: usize, target: &str) -> Result<(usize, usize), String> {
        let session = &self.sessions[session_index];
        let target = target.trim();
        if target.is_empty() || target == "{active}" || target == "@" {
            return session
                .windows
                .iter()
                .position(|window| window.index == session.active_window)
                .map(|index| (session_index, index))
                .ok_or_else(|| "window not found: active".to_owned());
        }
        if target == "^" || target == "{start}" {
            return session
                .windows
                .iter()
                .enumerate()
                .min_by_key(|(_, window)| window.index)
                .map(|(index, _)| (session_index, index))
                .ok_or_else(|| "no windows".to_owned());
        }
        if target == "$" || target == "{end}" {
            return session
                .windows
                .iter()
                .enumerate()
                .max_by_key(|(_, window)| window.index)
                .map(|(index, _)| (session_index, index))
                .ok_or_else(|| "no windows".to_owned());
        }
        if target == "!" || target == "{last}" {
            let index = session
                .last_window
                .ok_or_else(|| format!("window not found: {target}"))?;
            return session
                .windows
                .iter()
                .position(|window| window.index == index)
                .map(|position| (session_index, position))
                .ok_or_else(|| format!("window not found: {target}"));
        }
        if matches!(target, "+" | "-" | "{next}" | "{previous}")
            || target.starts_with('+')
            || target.starts_with('-')
        {
            let forward = target == "+" || target == "{next}" || target.starts_with('+');
            let count = target
                .strip_prefix('+')
                .or_else(|| target.strip_prefix('-'))
                .unwrap_or("")
                .parse::<usize>()
                .unwrap_or(1)
                .max(1);
            let current = session
                .windows
                .iter()
                .position(|window| window.index == session.active_window)
                .unwrap_or(0);
            let length = session.windows.len();
            if length == 0 {
                return Err("no windows".to_owned());
            }
            let position = if forward {
                (current + count) % length
            } else {
                (current + length - (count % length)) % length
            };
            return Ok((session_index, position));
        }
        if let Some(id) = target
            .strip_prefix('@')
            .and_then(|value| value.parse::<u64>().ok())
        {
            return session
                .windows
                .iter()
                .position(|window| window.id == id)
                .map(|index| (session_index, index))
                .ok_or_else(|| format!("window not found: @{id}"));
        }
        if let Ok(index) = target.parse::<u32>()
            && let Some(position) = session
                .windows
                .iter()
                .position(|window| window.index == index)
        {
            return Ok((session_index, position));
        }
        let exact = target.strip_prefix('=').unwrap_or(target);
        let matches = session
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| {
                if target.starts_with('=') {
                    window.name == exact
                } else {
                    wildcard_or_prefix(window.name.as_str(), exact)
                }
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [index] => Ok((session_index, *index)),
            [] => Err(format!("window not found: {target}")),
            _ => Err(format!("window target is ambiguous: {target}")),
        }
    }

    fn find_session(&self, name: &str) -> Option<usize> {
        let exact = name.strip_prefix('=').unwrap_or(name);
        if name.starts_with('=') {
            return self
                .sessions
                .iter()
                .position(|session| session.name == exact);
        }
        self.sessions
            .iter()
            .position(|session| session.name == name)
            .or_else(|| {
                let matches = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, session)| wildcard_or_prefix(&session.name, name))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    Some(matches[0])
                } else {
                    None
                }
            })
    }

    fn find_pane(&self, pane_id: u64) -> Option<&Pane> {
        self.sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| &window.panes)
            .find(|pane| pane.id == pane_id)
    }

    fn find_pane_location(&self, pane_id: u64) -> Option<(usize, usize, u64)> {
        for (session_index, session) in self.sessions.iter().enumerate() {
            for (window_index, window) in session.windows.iter().enumerate() {
                if window.pane(pane_id).is_some() {
                    return Some((session_index, window_index, pane_id));
                }
            }
        }
        None
    }

    fn next_session_name(&self) -> String {
        let mut index = 0;
        loop {
            let name = index.to_string();
            if self.sessions.iter().all(|session| session.name != name) {
                return name;
            }
            index += 1;
        }
    }

    fn next_pane_id(&mut self) -> u64 {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }
}

fn is_wide_copy_character(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
    )
}

fn parse_sgr_mouse(bytes: &[u8]) -> Option<(u16, u16, u16, bool)> {
    let last = *bytes.last()?;
    let body = bytes.strip_prefix(b"\x1b[<")?.strip_suffix(&[last])?;
    let mut fields = body.split(|byte| *byte == b';');
    let button = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    let col = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    let row = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    if fields.next().is_some() || !matches!(last, b'M' | b'm') {
        return None;
    }
    Some((button, col, row, last == b'm'))
}

fn encode_sgr_mouse(button: u16, col: u16, row: u16, release: bool) -> Vec<u8> {
    format!(
        "\x1b[<{};{};{}{}",
        button,
        col,
        row,
        if release { 'm' } else { 'M' }
    )
    .into_bytes()
}

fn encode_mouse_event(
    button: u16,
    col: u16,
    row: u16,
    release: bool,
    encoding: vt100::MouseProtocolEncoding,
) -> Vec<u8> {
    match encoding {
        vt100::MouseProtocolEncoding::Sgr => encode_sgr_mouse(button, col, row, release),
        vt100::MouseProtocolEncoding::Default | vt100::MouseProtocolEncoding::Utf8 => {
            let old_button = if release {
                (button & !0x03) | 0x03
            } else {
                button
            };
            let values = [
                old_button.saturating_add(32),
                col.saturating_add(32),
                row.saturating_add(32),
            ];
            let mut output = b"\x1b[M".to_vec();
            for value in values {
                let character = char::from_u32(u32::from(value)).unwrap_or('\u{fffd}');
                if encoding == vt100::MouseProtocolEncoding::Default {
                    output.push(u8::try_from(value).unwrap_or(b'?'));
                } else {
                    let mut bytes = [0; 4];
                    output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
                }
            }
            output
        }
    }
}

#[cfg(test)]
fn is_mouse_binding_name(value: &str) -> bool {
    value.starts_with("Mouse")
        || value.starts_with("Wheel")
        || value.starts_with("DoubleClick")
        || value.starts_with("TripleClick")
}

fn mouse_binding_name(button: u16, release: bool, click_count: u8) -> Option<String> {
    match button {
        64 => Some("WheelUpPane".to_owned()),
        65 => Some("WheelDownPane".to_owned()),
        button => {
            let motion = button & 0x20 != 0;
            let button = button & 0x03;
            if button > 2 {
                return None;
            }
            let number = button + 1;
            Some(if motion {
                format!("MouseDrag{number}Pane")
            } else if release {
                format!("MouseUp{number}Pane")
            } else if click_count == 2 {
                format!("DoubleClick{number}Pane")
            } else if click_count >= 3 {
                format!("TripleClick{number}Pane")
            } else {
                format!("MouseDown{number}Pane")
            })
        }
    }
}

fn parser_for_pane(pane: &Pane) -> Parser {
    let mut parser = Parser::new(pane.rect.rows.max(1), pane.rect.cols.max(1), 10_000);
    if pane.raw_output.is_empty() {
        parser.process(pane.parser.screen().contents().as_bytes());
    } else {
        terminal::replay(&mut parser, &pane.raw_output);
    }
    parser
}

/// Decode the CSI-u keyboard reports emitted after tm enables extended keys
/// on an attached terminal. The result is ordinary terminal input bytes so
/// the existing prefix, binding, prompt, and copy-mode state machines keep one
/// key representation. Incomplete CSI sequences stay buffered on the client
/// and are completed by the next input read.
fn decode_extended_key_input(input: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != 0x1b {
            output.push(input[index]);
            index += 1;
            continue;
        }
        if input.get(index + 1) != Some(&b'[') {
            output.push(input[index]);
            index += 1;
            continue;
        }
        let Some(final_offset) = input[index + 2..]
            .iter()
            .position(|byte| (0x40..=0x7e).contains(byte))
            .map(|offset| index + 2 + offset)
        else {
            return (output, input[index..].to_vec());
        };
        let end = final_offset + 1;
        let body = &input[index + 2..final_offset];
        let key = if input[final_offset] == b'u' {
            decode_csi_u(body)
        } else if input[final_offset] == b'~' {
            decode_xterm_extended_key(body)
        } else {
            None
        };
        if let Some(key) = key {
            output.extend_from_slice(&key);
        } else {
            output.extend_from_slice(&input[index..end]);
        }
        index = end;
    }
    (output, Vec::new())
}

fn decode_csi_u(body: &[u8]) -> Option<Vec<u8>> {
    let mut fields = body.split(|byte| *byte == b';');
    let codepoint = std::str::from_utf8(fields.next()?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    let modifier = fields
        .next()
        .and_then(|field| std::str::from_utf8(field).ok()?.parse::<u8>().ok())
        .unwrap_or(1);
    if fields.next().is_some() || !(1..=8).contains(&modifier) {
        return None;
    }
    decode_extended_key_codepoint(codepoint, modifier)
}

fn decode_xterm_extended_key(body: &[u8]) -> Option<Vec<u8>> {
    let mut fields = body.split(|byte| *byte == b';');
    let introducer = std::str::from_utf8(fields.next()?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    if introducer != 27 {
        return None;
    }
    let modifier = std::str::from_utf8(fields.next()?)
        .ok()?
        .parse::<u8>()
        .ok()?;
    let codepoint = std::str::from_utf8(fields.next()?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    if fields.next().is_some() {
        return None;
    }
    decode_extended_key_codepoint(codepoint, modifier)
}

fn decode_extended_key_codepoint(codepoint: u32, modifier: u8) -> Option<Vec<u8>> {
    if !(1..=8).contains(&modifier) {
        return None;
    }
    let control = modifier & 4 != 0;
    let meta = modifier & 2 != 0;
    let character = char::from_u32(codepoint)?;
    let mut encoded = [0; 4];
    let bytes = character.encode_utf8(&mut encoded).as_bytes();
    let mut output = Vec::with_capacity(bytes.len() + usize::from(meta));
    if meta {
        output.push(0x1b);
    }
    if control {
        let byte = u8::try_from(codepoint).ok()?;
        let control = match byte {
            b'@' | b' ' | b'`' => 0,
            0x7f => 0x08,
            b'a'..=b'z' | b'A'..=b'Z' => byte.to_ascii_uppercase() & 0x1f,
            b'['..=b'_' => byte & 0x1f,
            _ => return None,
        };
        output.push(control);
    } else {
        output.extend_from_slice(bytes);
    }
    Some(output)
}

/// Rebuild the isolated terminal view used by `copy-mode -s`. Ordinary copy
/// mode reads the target pane's live parser; a source-pane mode must leave
/// that parser untouched while still giving every action a scrollback view.
fn copy_source_parser(pane: &Pane, history_limit: usize) -> Option<Parser> {
    let source = pane.copy_source.as_ref()?;
    let mut parser = Parser::new(pane.rect.rows.max(1), pane.rect.cols.max(1), history_limit);
    terminal::replay(&mut parser, &source.raw_output);
    Some(parser)
}

fn mouse_word_at(line: &str, column: usize, separators: &str) -> String {
    let characters = line.chars().collect::<Vec<_>>();
    let Some(&character) = characters.get(column) else {
        return String::new();
    };
    if character.is_whitespace() || separators.contains(character) {
        return String::new();
    }
    let mut start = column;
    while start > 0 {
        let previous = characters[start - 1];
        if previous.is_whitespace() || separators.contains(previous) {
            break;
        }
        start -= 1;
    }
    let mut end = column + 1;
    while end < characters.len() {
        let next = characters[end];
        if next.is_whitespace() || separators.contains(next) {
            break;
        }
        end += 1;
    }
    characters[start..end].iter().collect()
}

fn mouse_hyperlink_at(output: &[u8], target_row: usize, target_col: usize, rect: Rect) -> String {
    let cols = usize::from(rect.cols.max(1));
    let rows = usize::from(rect.rows.max(1));
    let mut row = 0usize;
    let mut col = 0usize;
    let mut hyperlink = String::new();
    let mut index = 0usize;
    while index < output.len() {
        if output[index] == 0x1b
            && output.get(index + 1) == Some(&b']')
            && let Some((end, body)) = parse_osc_sequence(&output[index..])
        {
            if let Some(uri) = body
                .strip_prefix("8;")
                .and_then(|value| value.split_once(';').map(|(_, uri)| uri.to_owned()))
            {
                hyperlink = uri;
            }
            index += end;
            continue;
        }
        if output[index] == 0x1b
            && output.get(index + 1) == Some(&b'[')
            && let Some((end, body, final_byte)) = parse_csi_sequence(&output[index..])
        {
            apply_mouse_csi(&mut row, &mut col, body, final_byte, rows, cols);
            index += end;
            continue;
        }
        let byte = output[index];
        match byte {
            13 => col = 0,
            10 => {
                row = row.saturating_add(1).min(rows.saturating_sub(1));
                col = 0;
            }
            8 => col = col.saturating_sub(1),
            9 => col = ((col / 8).saturating_add(1)).saturating_mul(8).min(cols),
            0x20..=0x7e | 0xc0..=0xff => {
                let character = std::str::from_utf8(&output[index..])
                    .ok()
                    .and_then(|value| value.chars().next());
                let Some(character) = character else {
                    index += 1;
                    continue;
                };
                let width = if is_wide_copy_character(character) {
                    2
                } else {
                    1
                };
                if row == target_row && target_col >= col && target_col < col.saturating_add(width)
                {
                    return hyperlink;
                }
                col = col.saturating_add(width);
                if col >= cols {
                    row = row.saturating_add(1).min(rows.saturating_sub(1));
                    col = 0;
                }
                index += character.len_utf8();
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    String::new()
}

fn parse_osc_sequence(bytes: &[u8]) -> Option<(usize, String)> {
    if bytes.len() < 2 || bytes[0] != 0x1b || bytes[1] != b']' {
        return None;
    }
    let mut index = 2;
    while index < bytes.len() {
        if bytes[index] == 0x07 {
            return Some((
                index + 1,
                String::from_utf8_lossy(&bytes[2..index]).into_owned(),
            ));
        }
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&92) {
            return Some((
                index + 2,
                String::from_utf8_lossy(&bytes[2..index]).into_owned(),
            ));
        }
        index += 1;
    }
    None
}

fn parse_csi_sequence(bytes: &[u8]) -> Option<(usize, &str, u8)> {
    if bytes.len() < 2 || bytes[0] != 0x1b || bytes[1] != b'[' {
        return None;
    }
    let final_index = bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))?
        + 2;
    let body = std::str::from_utf8(&bytes[2..final_index]).ok()?;
    Some((final_index + 1, body, bytes[final_index]))
}

fn apply_mouse_csi(
    row: &mut usize,
    col: &mut usize,
    body: &str,
    final_byte: u8,
    rows: usize,
    cols: usize,
) {
    let values = body
        .trim_start_matches(['?', '>', '<'])
        .split(';')
        .filter_map(|value| value.parse::<usize>().ok())
        .collect::<Vec<_>>();
    let first = values
        .first()
        .copied()
        .filter(|value| *value > 0)
        .unwrap_or(1);
    match final_byte {
        b'H' | b'f' => {
            *row = values
                .first()
                .copied()
                .unwrap_or(1)
                .saturating_sub(1)
                .min(rows.saturating_sub(1));
            *col = values
                .get(1)
                .copied()
                .unwrap_or(1)
                .saturating_sub(1)
                .min(cols);
        }
        b'G' | 96 => *col = first.saturating_sub(1).min(cols),
        b'd' => *row = first.saturating_sub(1).min(rows.saturating_sub(1)),
        b'A' => *row = row.saturating_sub(first),
        b'B' => *row = row.saturating_add(first).min(rows.saturating_sub(1)),
        b'C' | b'a' => *col = col.saturating_add(first).min(cols),
        b'D' => *col = col.saturating_sub(first),
        b'E' => {
            *row = row.saturating_add(first).min(rows.saturating_sub(1));
            *col = 0;
        }
        b'F' => {
            *row = row.saturating_sub(first);
            *col = 0;
        }
        _ => {}
    }
}

fn previous_utf8_boundary(bytes: &[u8], cursor: usize) -> usize {
    let mut position = cursor.min(bytes.len()).saturating_sub(1);
    while position > 0 && (bytes[position] & 0xc0) == 0x80 {
        position -= 1;
    }
    position
}

fn terminal_input_sequence_len(bytes: &[u8]) -> usize {
    if bytes.len() < 2 || bytes[0] != 0x1b {
        return 1;
    }
    if matches!(bytes[1], b'[' | b'O') {
        return bytes[2..]
            .iter()
            .position(|byte| (0x40..=0x7e).contains(byte))
            .map_or(bytes.len(), |offset| offset + 3);
    }
    2.min(bytes.len())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[usize::from(first >> 2)] as char);
        output.push(TABLE[usize::from((first << 4 | second >> 4) & 0x3f)] as char);
        if chunk.len() > 1 {
            output.push(TABLE[usize::from((second << 2 | third >> 6) & 0x3f)] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[usize::from(third & 0x3f)] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn render_copy_mode_position(
    output: &mut Vec<u8>,
    rect: Rect,
    mode: &mut CopyModeState,
    parser: &mut Parser,
    format: &str,
    options: &HashMap<String, String>,
) {
    if mode.hide_position || format.is_empty() {
        return;
    }
    let position_limit = mode.position_limit(parser);
    if position_limit == 0 {
        return;
    }
    let values = [
        ("copy_position", mode.scroll_offset.to_string()),
        ("copy_position_limit", position_limit.to_string()),
        ("copy_cursor_x", mode.cursor_x.to_string()),
        ("copy_cursor_y", mode.cursor.row.to_string()),
        (
            "copy_line_numbers",
            if mode.line_numbers { "1" } else { "0" }.to_owned(),
        ),
        (
            "selection_present",
            if mode.selection_present() { "1" } else { "0" }.to_owned(),
        ),
        (
            "selection_active",
            if mode.selection_is_active() { "1" } else { "0" }.to_owned(),
        ),
        (
            "rectangle_toggle",
            if mode.rectangle_selection() { "1" } else { "0" }.to_owned(),
        ),
        (
            "refresh_active",
            if mode.refresh_active() { "1" } else { "0" }.to_owned(),
        ),
    ];
    let expanded = render_format_with_options(format, &values, options);
    let (alignment, expanded) = strip_copy_mode_alignment(&expanded);
    let plain = strip_status_styles(&expanded);
    let width = format_display_width(&plain);
    let cols = usize::from(rect.cols);
    let line_number_width = if mode.line_numbers {
        copy_mode_line_number_width_from_mode(mode, parser)
    } else {
        0
    };
    let content_cols = cols.saturating_sub(line_number_width);
    if width == 0 || width > content_cols {
        return;
    }
    let offset = match alignment {
        "left" => 0,
        "centre" | "center" => content_cols.saturating_sub(width) / 2,
        _ => content_cols.saturating_sub(width),
    } + line_number_width;
    output.extend_from_slice(
        format!(
            "\x1b[{};{}H",
            rect.y.saturating_add(1),
            rect.x
                .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX))
                + 1
        )
        .as_bytes(),
    );
    output.extend_from_slice(render_status_styles(&expanded).as_bytes());
    output.extend_from_slice(b"\x1b[K");
}

fn copy_mode_line_number_width_from_mode(mode: &mut CopyModeState, parser: &mut Parser) -> usize {
    let lines = mode
        .position_limit(parser)
        .saturating_add(usize::from(parser.screen().size().0))
        .saturating_add(1);
    let mut digits: usize = 1;
    let mut value = lines;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits.max(3).saturating_add(1)
}

fn strip_copy_mode_alignment(value: &str) -> (&str, String) {
    for alignment in ["left", "centre", "center", "right"] {
        let marker = format!("#[align={alignment}]");
        if let Some(stripped) = value.strip_prefix(&marker) {
            return (alignment, stripped.to_owned());
        }
    }
    ("right", value.to_owned())
}

fn effective_pane_option<'a>(
    pane: &'a Pane,
    window_options: &'a HashMap<String, String>,
    global: &'a HashMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    pane.options
        .get(key)
        .or_else(|| window_options.get(key))
        .or_else(|| global.get(key))
        .map(String::as_str)
}

fn scrollbar_colour_code(value: &str, background: bool) -> u8 {
    let base = if background { 40 } else { 30 };
    match value {
        "black" => base,
        "red" => base + 1,
        "green" => base + 2,
        "yellow" => base + 3,
        "blue" => base + 4,
        "magenta" => base + 5,
        "cyan" => base + 6,
        "white" => base + 7,
        "brightblack" | "grey" | "gray" => base + 60,
        "brightred" => base + 61,
        "brightgreen" => base + 62,
        "brightyellow" => base + 63,
        "brightblue" => base + 64,
        "brightmagenta" => base + 65,
        "brightcyan" => base + 66,
        "brightwhite" => base + 67,
        // Themed colours need a terminal palette that tm does not own;
        // use the closest stable ANSI fallback for headless rendering.
        "themedarkgrey" | "themedarkgray" | "default" => {
            if background {
                40
            } else {
                30
            }
        }
        "themelightgrey" | "themelightgray" => {
            if background {
                47
            } else {
                37
            }
        }
        _ => {
            if background {
                40
            } else {
                37
            }
        }
    }
}

fn scrollbar_style_codes(bg: &str, fg: &str, slider: bool) -> (u8, u8) {
    if slider {
        (
            scrollbar_colour_code(fg, true),
            scrollbar_colour_code(bg, false),
        )
    } else {
        (
            scrollbar_colour_code(bg, true),
            scrollbar_colour_code(fg, false),
        )
    }
}

#[derive(Clone, Copy)]
struct PaneScrollbarLayout {
    position_left: bool,
    width: usize,
    pad: usize,
    slider_top: usize,
    slider_rows: usize,
    reserve_content: bool,
}

fn parse_pane_scrollbar_style(style: &str) -> (&str, &str, usize, usize) {
    let mut bg = "themedarkgrey";
    let mut fg = "themelightgrey";
    let mut width = 1usize;
    let mut pad = 0usize;
    for part in style.split(',').map(str::trim) {
        if let Some(value) = part.strip_prefix("bg=") {
            bg = value;
        } else if let Some(value) = part.strip_prefix("fg=") {
            fg = value;
        } else if let Some(value) = part.strip_prefix("width=") {
            width = value.parse().unwrap_or(1);
        } else if let Some(value) = part.strip_prefix("pad=") {
            pad = value.parse().unwrap_or(0);
        }
    }
    (bg, fg, width.max(1), pad)
}

fn pane_scrollbar_layout(
    pane: &Pane,
    window_options: &HashMap<String, String>,
    global: &HashMap<String, String>,
    history_limit: usize,
) -> Option<PaneScrollbarLayout> {
    let scrollbar_mode =
        effective_pane_option(pane, window_options, global, "pane-scrollbars").unwrap_or("off");
    let in_copy_mode = pane.copy_mode.is_some();
    let visible = match scrollbar_mode {
        "on" => true,
        "modal" | "auto-hide" => in_copy_mode,
        _ => false,
    };
    if !visible || pane.rect.cols == 0 || pane.rect.rows == 0 {
        return None;
    }

    let mut parser =
        copy_source_parser(pane, history_limit).unwrap_or_else(|| parser_for_pane(pane));
    let (history, live) = history_rows(&mut parser);
    let total_rows = history.len().saturating_add(live.len());
    let viewport_rows = usize::from(parser.screen().size().0).max(1);
    let max_scroll = total_rows.saturating_sub(viewport_rows);
    if max_scroll == 0 {
        return None;
    }
    let scroll_offset = pane
        .copy_mode
        .as_ref()
        .map_or(0, |mode| mode.scroll_position())
        .min(max_scroll);
    let (slider_top, slider_rows) =
        scrollbar_geometry_for(total_rows, viewport_rows, max_scroll, scroll_offset);

    let position = effective_pane_option(pane, window_options, global, "pane-scrollbars-position")
        .unwrap_or("right");
    let style = effective_pane_option(pane, window_options, global, "pane-scrollbars-style")
        .unwrap_or("bg=themedarkgrey,fg=themelightgrey,width=1,pad=0");
    let (_, _, requested_width, requested_pad) = parse_pane_scrollbar_style(style);
    let pane_cols = usize::from(pane.rect.cols);
    let width = requested_width.min(pane_cols).max(1);
    let pad = requested_pad.min(pane_cols.saturating_sub(width));
    Some(PaneScrollbarLayout {
        position_left: position == "left",
        width,
        pad,
        slider_top,
        slider_rows,
        reserve_content: scrollbar_mode == "on",
    })
}

/// Return the pane area available to terminal content after an always-on
/// scrollbar reserves its track and pad. Modal scrollbars intentionally float
/// over the pane, matching tmux's copy-mode-only overlay behavior.
fn pane_content_rect(
    pane: &Pane,
    window_options: &HashMap<String, String>,
    global: &HashMap<String, String>,
    history_limit: usize,
) -> Rect {
    let Some(layout) = pane_scrollbar_layout(pane, window_options, global, history_limit) else {
        return pane.rect;
    };
    if !layout.reserve_content {
        return pane.rect;
    }
    let reserved = layout.width.saturating_add(layout.pad);
    if layout.position_left {
        Rect {
            x: pane
                .rect
                .x
                .saturating_add(u16::try_from(reserved).unwrap_or(u16::MAX)),
            cols: pane
                .rect
                .cols
                .saturating_sub(u16::try_from(reserved).unwrap_or(u16::MAX)),
            ..pane.rect
        }
    } else {
        Rect {
            cols: pane
                .rect
                .cols
                .saturating_sub(u16::try_from(reserved).unwrap_or(u16::MAX)),
            ..pane.rect
        }
    }
}

fn render_pane_scrollbar(
    output: &mut Vec<u8>,
    pane: &Pane,
    window_options: &HashMap<String, String>,
    global: &HashMap<String, String>,
    history_limit: usize,
) {
    let Some(layout) = pane_scrollbar_layout(pane, window_options, global, history_limit) else {
        return;
    };
    let style = effective_pane_option(pane, window_options, global, "pane-scrollbars-style")
        .unwrap_or("bg=themedarkgrey,fg=themelightgrey,width=1,pad=0");
    let (bg, fg, _, _) = parse_pane_scrollbar_style(style);
    let pane_x = usize::from(pane.rect.x);
    let pane_y = usize::from(pane.rect.y);
    let pane_cols = usize::from(pane.rect.cols);
    let scrollbar_x = if layout.position_left {
        pane_x
    } else {
        pane_x.saturating_add(pane_cols.saturating_sub(layout.width))
    };
    let pad_x = if layout.position_left {
        scrollbar_x.saturating_add(layout.width)
    } else {
        scrollbar_x.saturating_sub(layout.pad)
    };
    let (track_bg, track_fg) = scrollbar_style_codes(bg, fg, false);
    let (slider_bg, slider_fg) = scrollbar_style_codes(bg, fg, true);
    for row in 0..usize::from(pane.rect.rows) {
        let terminal_row = pane_y.saturating_add(row).saturating_add(1);
        if layout.pad > 0 {
            output.extend_from_slice(
                format!(
                    "\x1b[{terminal_row};{}H\x1b[0m{}",
                    pad_x.saturating_add(1),
                    " ".repeat(layout.pad)
                )
                .as_bytes(),
            );
        }
        let slider =
            row >= layout.slider_top && row < layout.slider_top.saturating_add(layout.slider_rows);
        let (background, foreground) = if slider {
            (slider_bg, slider_fg)
        } else {
            (track_bg, track_fg)
        };
        output.extend_from_slice(
            format!(
                "\x1b[{terminal_row};{}H\x1b[{background}m\x1b[{foreground}m{}\x1b[0m",
                scrollbar_x.saturating_add(1),
                " ".repeat(layout.width)
            )
            .as_bytes(),
        );
    }
}

fn strip_status_styles(value: &str) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < value.len() {
        if value[index..].starts_with("#[")
            && let Some(end) = value[index + 2..].find(']')
        {
            index += end + 3;
            continue;
        }
        let character = value[index..]
            .chars()
            .next()
            .expect("format index is on a character boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellStyle {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

fn render_layout_separators(output: &mut Vec<u8>, window: &Window) {
    if window.zoomed {
        return;
    }
    let mut separators = Vec::new();
    window.layout.separators(
        Rect {
            x: 0,
            y: 0,
            cols: window.size.cols,
            rows: window.size.rows,
        },
        &mut separators,
    );
    // Keep the escape sequence order stable. A fresh HashMap has a fresh
    // randomized iteration seed, which made otherwise identical frames differ
    // byte-for-byte and defeated the attach loop's redraw suppression.
    let mut cells = BTreeMap::<(u16, u16), u8>::new();
    for (x, y, axis) in separators {
        let bit = match axis {
            Axis::Horizontal => 1,
            Axis::Vertical => 2,
        };
        cells
            .entry((x, y))
            .and_modify(|value| *value |= bit)
            .or_insert(bit);
    }
    for ((x, y), kind) in cells {
        let glyph = match kind {
            1 => "│",
            2 => "─",
            _ => "┼",
        };
        let active = separator_touches_active_pane(window, x, y, kind);
        let active_color = window
            .pane(window.active_pane)
            .map(|pane| {
                if pane.copy_mode.is_some() {
                    33
                } else if window.synchronize_panes {
                    31
                } else {
                    32
                }
            })
            .unwrap_or(32);
        let style = active
            .then_some(active_color)
            .map_or_else(String::new, |color| format!("\x1b[{color}m"));
        let reset = if active { "\x1b[39m" } else { "" };
        output.extend_from_slice(
            format!(
                "\x1b[{};{}H\x1b[0m{}{}{}",
                y.saturating_add(1),
                x.saturating_add(1),
                style,
                glyph,
                reset
            )
            .as_bytes(),
        );
    }
}

fn separator_touches_active_pane(window: &Window, x: u16, y: u16, kind: u8) -> bool {
    let Some(active) = window.pane(window.active_pane).map(|pane| pane.rect) else {
        return false;
    };
    match kind {
        1 => {
            let right_edge = active.x.saturating_add(active.cols);
            (x == active.x.saturating_sub(1) || x == right_edge)
                && y >= active.y
                && y < active.y.saturating_add(active.rows)
        }
        2 => {
            let bottom_edge = active.y.saturating_add(active.rows);
            (y == active.y.saturating_sub(1) || y == bottom_edge)
                && x >= active.x
                && x < active.x.saturating_add(active.cols)
        }
        _ => {
            separator_touches_active_pane(window, x, y, 1)
                || separator_touches_active_pane(window, x, y, 2)
        }
    }
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

impl CellStyle {
    fn from_cell(cell: &vt100::Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }
}

fn append_color(output: &mut Vec<u8>, color: Color, foreground: bool) {
    let (default_code, basic_base, bright_base, extended_prefix) = if foreground {
        (39, 30, 90, 38)
    } else {
        (49, 40, 100, 48)
    };
    match color {
        Color::Default => output.extend_from_slice(format!("\x1b[{default_code}m").as_bytes()),
        Color::Idx(index) if index < 8 => {
            output.extend_from_slice(format!("\x1b[{}m", basic_base + u16::from(index)).as_bytes())
        }
        Color::Idx(index) if index < 16 => output
            .extend_from_slice(format!("\x1b[{}m", bright_base + u16::from(index - 8)).as_bytes()),
        Color::Idx(index) => {
            output.extend_from_slice(format!("\x1b[{extended_prefix};5;{index}m").as_bytes())
        }
        Color::Rgb(red, green, blue) => output.extend_from_slice(
            format!("\x1b[{extended_prefix};2;{red};{green};{blue}m").as_bytes(),
        ),
    }
}

fn append_cell_style(output: &mut Vec<u8>, current: &mut CellStyle, style: CellStyle) {
    if *current == style {
        return;
    }
    output.extend_from_slice(b"\x1b[0m");
    if style.fg != Color::Default {
        append_color(output, style.fg, true);
    }
    if style.bg != Color::Default {
        append_color(output, style.bg, false);
    }
    if style.bold {
        output.extend_from_slice(b"\x1b[1m");
    }
    if style.dim {
        output.extend_from_slice(b"\x1b[2m");
    }
    if style.italic {
        output.extend_from_slice(b"\x1b[3m");
    }
    if style.underline {
        output.extend_from_slice(b"\x1b[4m");
    }
    if style.inverse {
        output.extend_from_slice(b"\x1b[7m");
    }
    *current = style;
}

/// Turn a fully rendered terminal state into the smallest safe update for an
/// already synchronized terminal. Rows are rewritten atomically from column
/// one, which avoids the visible clear-and-repaint flash while keeping stale
/// tails erased with `EL`.
fn render_screen_delta(
    previous: &vt100::Screen,
    current: &vt100::Screen,
) -> Result<Option<Vec<u8>>, ()> {
    if previous.size() != current.size()
        || previous.alternate_screen() != current.alternate_screen()
        || previous.application_cursor() != current.application_cursor()
        || previous.application_keypad() != current.application_keypad()
        || previous.bracketed_paste() != current.bracketed_paste()
        || previous.mouse_protocol_mode() != current.mouse_protocol_mode()
        || previous.mouse_protocol_encoding() != current.mouse_protocol_encoding()
    {
        return Err(());
    }
    let (rows, cols) = current.size();
    let changed_rows = (0..rows)
        .filter(|row| (0..cols).any(|col| previous.cell(*row, col) != current.cell(*row, col)))
        .collect::<Vec<_>>();
    let changed = !changed_rows.is_empty();
    let cursor_changed = previous.cursor_position() != current.cursor_position()
        || previous.hide_cursor() != current.hide_cursor();
    let mut output = Vec::new();
    // Match tmux's redraw discipline: suppress a visible cursor before any
    // changed rows are written, then restore the final cursor state once.
    // When the cursor was already hidden, avoid emitting an idempotent hide.
    if changed && !previous.hide_cursor() {
        output.extend_from_slice(b"\x1b[?25l");
    }
    for row in changed_rows {
        output.extend_from_slice(format!("\x1b[{};1H", row.saturating_add(1)).as_bytes());
        let mut cell_style = CellStyle::default();
        for col in 0..cols {
            let Some(cell) = current.cell(row, col) else {
                output.push(b' ');
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            append_cell_style(&mut output, &mut cell_style, CellStyle::from_cell(cell));
            if cell.contents().is_empty() {
                output.push(b' ');
            } else {
                output.extend_from_slice(cell.contents().as_bytes());
            }
        }
        output.extend_from_slice(b"\x1b[0m\x1b[K");
    }
    if changed && !current.hide_cursor() {
        let (row, col) = current.cursor_position();
        output.extend_from_slice(
            format!(
                "\x1b[{};{}H\x1b[?25h",
                row.saturating_add(1),
                col.saturating_add(1)
            )
            .as_bytes(),
        );
    } else if !changed && cursor_changed {
        if current.hide_cursor() {
            if !previous.hide_cursor() {
                output.extend_from_slice(b"\x1b[?25l");
            }
        } else {
            let (row, col) = current.cursor_position();
            output.extend_from_slice(
                format!("\x1b[{};{}H", row.saturating_add(1), col.saturating_add(1))
                    .as_bytes(),
            );
            if previous.hide_cursor() {
                output.extend_from_slice(b"\x1b[?25h");
            }
        }
    }
    Ok((!output.is_empty()).then_some(output))
}

/// Restore a status prompt's editing cursor after its row has been painted.
/// The status prompt owns the client cursor while command or copy-mode input
/// is active; otherwise the active pane owns it.
fn render_status_prompt_cursor(
    output: &mut Vec<u8>,
    size: Size,
    options: &HashMap<String, String>,
    prompt: &str,
) {
    let row = if options
        .get("status-position")
        .is_some_and(|value| value == "top")
    {
        1
    } else {
        size.rows.max(1)
    };
    let col = format_display_width(prompt)
        .min(usize::from(size.cols.max(1)).saturating_sub(1));
    output.extend_from_slice(format!("\x1b[{row};{}H\x1b[?25h", col + 1).as_bytes());
}

fn render_pane_prompt_cursor(
    output: &mut Vec<u8>,
    rect: Rect,
    options: &HashMap<String, String>,
    prompt: &str,
) {
    let status_enabled = !options
        .get("status")
        .is_some_and(|value| !parse_on_off(value).unwrap_or(true));
    let row = rect.y + if status_enabled {
        rect.rows.saturating_sub(1)
    } else {
        rect.rows
    };
    let col = format_display_width(prompt)
        .min(usize::from(rect.cols.max(1)).saturating_sub(1));
    output.extend_from_slice(
        format!(
            "\x1b[{};{}H\x1b[?25h",
            row,
            rect.x + u16::try_from(col).unwrap_or(u16::MAX) + 1
        )
        .as_bytes(),
    );
}

fn render_status_line(
    output: &mut Vec<u8>,
    session: &Session,
    active_window: &Window,
    client_prefix: bool,
    options: &HashMap<String, String>,
    message: Option<&str>,
    prompt: Option<&str>,
) {
    if options
        .get("status")
        .is_some_and(|value| !parse_on_off(value).unwrap_or(true))
    {
        return;
    }
    let row = if options
        .get("status-position")
        .is_some_and(|value| value == "top")
    {
        1
    } else {
        session.size.rows.max(1)
    };
    output.extend_from_slice(format!("\x1b[{row};1H\x1b[K").as_bytes());
    output.extend_from_slice(match options.get("status-bg").map(String::as_str) {
        Some("black") | None => b"\x1b[40m",
        Some("red") => b"\x1b[41m",
        Some("green") => b"\x1b[42m",
        Some("blue") => b"\x1b[44m",
        _ => b"\x1b[40m",
    });
    output.extend_from_slice(match options.get("status-fg").map(String::as_str) {
        Some("black") => b"\x1b[30m",
        Some("red") => b"\x1b[31m",
        Some("green") => b"\x1b[32m",
        Some("blue") => b"\x1b[34m",
        Some("white") | None => b"\x1b[37m",
        _ => b"\x1b[37m",
    });
    if let Some(message) = message.or(prompt) {
        output.extend_from_slice(message.as_bytes());
        output.extend_from_slice(b"\x1b[0m");
        return;
    }
    let left_format = options.get("status-left").map_or("", String::as_str);
    let left = render_status_format(left_format, session, active_window, client_prefix, options);
    let status_window_format = options
        .get("window-status-format")
        .map_or("#I:#W", String::as_str);
    let current_window_format = options
        .get("window-status-current-format")
        .map_or("#I:#W", String::as_str);
    let mut windows = String::new();
    for window in &session.windows {
        if !windows.is_empty() {
            windows.push(' ');
        }
        let format = if window.index == session.active_window {
            current_window_format
        } else {
            status_window_format
        };
        windows.push_str(&render_status_format(
            format,
            session,
            window,
            client_prefix,
            options,
        ));
    }
    let right_format = options.get("status-right").map_or("", String::as_str);
    let right = render_status_format(right_format, session, active_window, client_prefix, options);
    let left_length = options
        .get("status-left-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(32);
    let left = take_display_width(&left, left_length, false);
    output.extend_from_slice(left.as_bytes());
    if !windows.is_empty() {
        output.push(b' ');
        output.extend_from_slice(windows.as_bytes());
    }
    if !right.is_empty() {
        output.extend_from_slice(b"  ");
        output.extend_from_slice(right.as_bytes());
    }
    output.extend_from_slice(b"\x1b[0m");
}

fn window_alert_flags(window: &Window) -> String {
    if window.bell_alert {
        "!".to_owned()
    } else {
        String::new()
    }
}

fn render_status_format(
    format: &str,
    session: &Session,
    window: &Window,
    client_prefix: bool,
    options: &HashMap<String, String>,
) -> String {
    let format = format
        .replace("#I", &window.index.to_string())
        .replace("#W", &window.name)
        .replace("#S", &session.name);
    let values = [
        ("session_id", format!("${}", session.id)),
        ("session_name", session.name.clone()),
        ("window_index", window.index.to_string()),
        ("window_id", format!("@{}", window.id)),
        ("window_name", window.name.clone()),
        (
            "window_zoomed_flag",
            if window.zoomed { "1" } else { "0" }.to_owned(),
        ),
        (
            "window_bell_flag",
            if window.bell_alert { "1" } else { "0" }.to_owned(),
        ),
        ("window_flags", window_alert_flags(window)),
        (
            "window_active",
            if window.index == session.active_window {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "client_prefix",
            if client_prefix { "1" } else { "0" }.to_owned(),
        ),
    ];
    let expanded = render_format_with_options(&format, &values, options);
    render_status_styles(&expanded)
}

fn render_status_styles(value: &str) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < value.len() {
        if value[index..].starts_with("#[")
            && let Some(end) = value[index + 2..].find(']')
        {
            let style = &value[index + 2..index + 2 + end];
            output.push_str(match style {
                "dim" => "\x1b[2m",
                "fg=green" => "\x1b[32m",
                "fg=yellow" => "\x1b[33m",
                "fg=red" => "\x1b[31m",
                "fg=white" => "\x1b[37m",
                "default" => "\x1b[0m",
                _ => "",
            });
            index += end + 3;
            continue;
        }
        let character = value[index..]
            .chars()
            .next()
            .expect("status format index is on a character boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn wildcard_or_prefix(value: &str, pattern: &str) -> bool {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        value.starts_with(prefix)
            && value.ends_with(suffix)
            && value.len() >= prefix.len() + suffix.len()
    } else {
        value == pattern || value.starts_with(pattern)
    }
}

fn parse_on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => Err(format!("expected on or off, got: {value}")),
    }
}

fn parse_copy_line_number_mode(value: &str) -> CopyLineNumberMode {
    match value {
        "default" => CopyLineNumberMode::Default,
        "absolute" | "on" | "yes" | "true" | "1" => CopyLineNumberMode::Absolute,
        "relative" => CopyLineNumberMode::Relative,
        "hybrid" => CopyLineNumberMode::Hybrid,
        "off" | "no" | "false" | "0" => CopyLineNumberMode::Off,
        _ => CopyLineNumberMode::Off,
    }
}

fn capture_absolute_offset(offset: i32, history_rows: usize) -> usize {
    if offset < 0 {
        history_rows.saturating_sub(offset.unsigned_abs() as usize)
    } else {
        history_rows.saturating_add(offset as usize)
    }
}

fn render_tree_mode_overlay(output: &mut Vec<u8>, mode: &TreeMode, size: Size) {
    output.extend_from_slice(b"\x1b[?25l");
    let rows = usize::from(size.rows.max(1));
    let cols = usize::from(size.cols.max(1));
    let visible = rows.saturating_sub(1);
    for (index, entry) in mode.entries.iter().take(visible).enumerate() {
        let row = index + 1;
        let text = if format_display_width(&entry.text) > cols {
            take_display_width(&entry.text, cols, false)
        } else {
            entry.text.clone()
        };
        output.extend_from_slice(format!("\x1b[{row};1H\x1b[K").as_bytes());
        if index == mode.cursor {
            output.extend_from_slice(b"\x1b[7m");
        }
        output.extend_from_slice(text.as_bytes());
        if index == mode.cursor {
            output.extend_from_slice(b"\x1b[27m");
        }
    }
    let footer = if let Some(label) = mode.confirmation_label.as_ref() {
        format!("{label}? (y/n)")
    } else if let Some(input) = mode.filter_input.as_ref() {
        format!("filter: {}", String::from_utf8_lossy(input))
    } else if mode.no_matches {
        "no matches".to_owned()
    } else {
        String::new()
    };
    if !footer.is_empty() {
        let footer = if format_display_width(&footer) > cols {
            take_display_width(&footer, cols, false)
        } else {
            footer
        };
        output.extend_from_slice(format!("\x1b[{rows};1H\x1b[K{footer}").as_bytes());
    }
}

fn render_buffer_mode_overlay(output: &mut Vec<u8>, mode: &BufferMode, size: Size) {
    let footer = mode
        .filter_input
        .as_ref()
        .map(|input| format!("filter: {}", String::from_utf8_lossy(input)))
        .or_else(|| mode.no_matches.then_some("no matches".to_owned()));
    let texts = mode
        .entries
        .iter()
        .map(|entry| entry.text.clone())
        .collect::<Vec<_>>();
    render_list_mode_overlay(output, &texts, mode.cursor, footer.as_deref(), size);
}

fn render_client_mode_overlay(output: &mut Vec<u8>, mode: &ClientMode, size: Size) {
    let footer = mode
        .filter_input
        .as_ref()
        .map(|input| format!("filter: {}", String::from_utf8_lossy(input)))
        .or_else(|| mode.no_matches.then_some("no matches".to_owned()));
    let texts = mode
        .entries
        .iter()
        .map(|entry| entry.text.clone())
        .collect::<Vec<_>>();
    render_list_mode_overlay(output, &texts, mode.cursor, footer.as_deref(), size);
}

fn render_panes_mode_overlay(output: &mut Vec<u8>, mode: &PaneDisplayMode, size: Size) {
    let texts = mode
        .entries
        .iter()
        .map(|entry| entry.text.clone())
        .collect::<Vec<_>>();
    render_list_mode_overlay(output, &texts, usize::MAX, None, size);
}

fn render_list_mode_overlay(
    output: &mut Vec<u8>,
    texts: &[String],
    cursor: usize,
    footer: Option<&str>,
    size: Size,
) {
    output.extend_from_slice(b"\x1b[?25l");
    let rows = usize::from(size.rows.max(1));
    let cols = usize::from(size.cols.max(1));
    let visible = rows.saturating_sub(1);
    for (index, text) in texts.iter().take(visible).enumerate() {
        let row = index + 1;
        let text = if format_display_width(text) > cols {
            take_display_width(text, cols, false)
        } else {
            text.clone()
        };
        output.extend_from_slice(format!("\x1b[{row};1H\x1b[K").as_bytes());
        if index == cursor {
            output.extend_from_slice(b"\x1b[7m");
        }
        output.extend_from_slice(text.as_bytes());
        if index == cursor {
            output.extend_from_slice(b"\x1b[27m");
        }
    }
    if let Some(footer) = footer {
        let footer = if format_display_width(footer) > cols {
            take_display_width(footer, cols, false)
        } else {
            footer.to_owned()
        };
        output.extend_from_slice(format!("\x1b[{rows};1H\x1b[K{footer}").as_bytes());
    }
}

fn capture_formatted_row(
    parser: &mut Parser,
    history_rows: usize,
    row: usize,
    raw_output: &[u8],
) -> Vec<u8> {
    if let Some(raw_row) = raw_hyperlink_row(raw_output, row) {
        return raw_row;
    }
    let saved = parser.screen().scrollback();
    let (rows, cols) = parser.screen().size();
    let formatted = if row < history_rows {
        parser
            .screen_mut()
            .set_scrollback(history_rows.saturating_sub(row));
        parser
            .screen()
            .rows_formatted(0, cols)
            .next()
            .unwrap_or_default()
    } else {
        parser.screen_mut().set_scrollback(0);
        parser
            .screen()
            .rows_formatted(0, cols)
            .nth(row.saturating_sub(history_rows).min(usize::from(rows)))
            .unwrap_or_default()
    };
    parser.screen_mut().set_scrollback(saved);
    formatted
}

fn raw_hyperlink_row(raw_output: &[u8], target_row: usize) -> Option<Vec<u8>> {
    // vt100 intentionally drops OSC 8 while retaining the displayed cells.
    // For direct PTY rows, the retained stream is therefore the only source
    // that can reproduce the hyperlink controls. Rows without OSC 8 continue
    // through the normal attribute-aware formatter above.
    raw_output
        .split(|byte| *byte == b'\n')
        .nth(target_row)
        .filter(|row| row.windows(4).any(|window| window == b"\x1b]8;"))
        .map(|row| row.strip_suffix(b"\r").unwrap_or(row).to_vec())
}

fn execute_request(
    state: &mut ServerState,
    shared: &SharedState,
    request: Request,
) -> CommandResult {
    match request {
        Request::NewSession {
            name,
            detached: _,
            attach_existing,
            group_target,
            format,
            window_name,
            empty,
            command,
            cwd,
            size,
        } => state.create_session(
            shared,
            name.as_deref(),
            attach_existing,
            group_target.as_deref(),
            format.as_deref(),
            window_name.as_deref(),
            empty,
            &command,
            cwd.as_deref(),
            size,
        ),
        Request::ListSessions { format } => Ok(state.list_sessions(format.as_deref())),
        Request::ListClients { format } => state.list_clients(format.as_deref()),
        Request::DetachClient { target, all } => state.detach_client(target.as_deref(), all),
        Request::SwitchClient { client, session } => {
            state.switch_client(client.as_deref(), &session)
        }
        Request::RefreshClient { target } => state.refresh_client(target.as_deref()),
        Request::RunShell {
            command,
            background,
            target,
        } => state.run_shell(&command, background, target.as_deref()),
        Request::HasSession { target } => state
            .find_session(&target)
            .map(|_| String::new())
            .ok_or_else(|| format!("session not found: {target}")),
        Request::KillSession { target, all } => state.kill_session(target.as_deref(), all),
        Request::NewWindow {
            target,
            name,
            detached,
            empty,
            after,
            before,
            select_existing,
            index,
            force,
            format,
            command,
            cwd,
        } => state.new_window(
            shared,
            target.as_deref(),
            name.as_deref(),
            detached,
            index,
            force,
            format.as_deref(),
            after,
            before,
            select_existing,
            empty,
            &command,
            cwd.as_deref(),
        ),
        Request::SplitWindow {
            target,
            horizontal,
            before,
            full,
            detached,
            zoom,
            empty,
            size,
            command,
            cwd,
        } => state.split_window(
            shared,
            target.as_deref(),
            horizontal,
            before,
            full,
            detached,
            zoom,
            empty,
            size.as_deref(),
            &command,
            cwd.as_deref(),
        ),
        Request::ListWindows { target, format } => {
            state.list_windows(target.as_deref(), format.as_deref())
        }
        Request::ListPanes { target, format } => {
            state.list_panes(target.as_deref(), format.as_deref())
        }
        Request::SelectWindow { target } => state.select_window(&target),
        Request::RotateWindow { target, up } => state.rotate_window(target.as_deref(), up),
        Request::SwapWindow {
            source,
            target,
            detached,
        } => state.swap_window(source.as_deref(), target.as_deref(), detached),
        Request::MoveWindow {
            source,
            target,
            after,
            detached,
            force,
            renumber,
        } => state.move_window(
            source.as_deref(),
            target.as_deref(),
            after,
            detached,
            force,
            renumber,
        ),
        Request::LinkWindow {
            source,
            target,
            detached,
            force,
        } => state.link_window(source.as_deref(), target.as_deref(), detached, force),
        Request::UnlinkWindow { target, force } => state.unlink_window(target.as_deref(), force),
        Request::NextWindow { target } => state.next_window(target.as_deref()),
        Request::PreviousWindow { target } => state.previous_window(target.as_deref()),
        Request::RenameSession { target, name } => state.rename_session(target.as_deref(), &name),
        Request::RenameWindow { target, name } => state.rename_window(target.as_deref(), &name),
        Request::KillWindow { target, all } => state.kill_window(target.as_deref(), all),
        Request::SelectPane {
            target,
            direction,
            mark,
            title,
            enabled,
        } => state.select_pane(target.as_deref(), direction, mark, title, enabled),
        Request::KillPane {
            target,
            all,
            filter,
        } => state.kill_pane(target.as_deref(), all, filter.as_deref()),
        Request::ResizePane {
            target,
            direction,
            amount,
            absolute,
            absolute_percent,
            zoom,
        } => state.resize_pane(
            target.as_deref(),
            direction,
            amount,
            absolute,
            absolute_percent,
            zoom,
        ),
        Request::SwapPane {
            source,
            target,
            direction,
            detached,
        } => state.swap_pane(source.as_deref(), target.as_deref(), direction, detached),
        Request::BreakPane {
            source,
            target,
            name,
            detached,
            format,
        } => state.break_pane(
            source.as_deref(),
            target.as_deref(),
            name.as_deref(),
            detached,
            format.as_deref(),
        ),
        Request::JoinPane {
            source,
            target,
            horizontal,
            before,
            detached,
            size,
        } => state.join_pane(
            source.as_deref(),
            target.as_deref(),
            horizontal,
            before,
            detached,
            size.as_deref(),
        ),
        Request::RespawnPane {
            target,
            command,
            cwd,
            kill,
            empty,
            window,
        } => state.respawn_pane(
            shared,
            target.as_deref(),
            &command,
            cwd.as_deref(),
            kill,
            empty,
            window,
        ),
        Request::ClearHistory { target } => {
            let (_, _, pane_id) = state.resolve_pane_target(target.as_deref())?;
            let pane = state
                .find_pane_mut(pane_id)
                .ok_or_else(|| "target pane no longer exists".to_owned())?;
            let saved_scrollback = pane.parser.screen().scrollback();
            pane.parser.screen_mut().set_scrollback(usize::MAX);
            pane.history_floor = pane.parser.screen().scrollback();
            pane.parser.screen_mut().set_scrollback(saved_scrollback);
            pane.parser.screen_mut().set_scrollback(0);
            // Keep a replayable checkpoint instead of leaving raw_output empty:
            // later copy/format paths must reconstruct the same visible state
            // (including cursor and terminal modes) as the live parser.
            checkpoint_raw_output(pane);
            pane.copy_mode = None;
            pane.copy_source = None;
            Ok(String::new())
        }
        Request::SendKeys {
            target,
            bytes,
            reset,
        } => {
            let (_, _, pane_id) = state.resolve_pane_target(target.as_deref())?;
            if state.find_pane(pane_id).is_none() {
                return Err("target pane no longer exists".to_owned());
            }
            if reset && let Some(pane) = state.find_pane_mut(pane_id) {
                pane.copy_mode = None;
                pane.copy_source = None;
                pane.output_state = terminal::OutputState::default();
                pane.parser.process(b"\x1bc");
            }
            state.write_pane(pane_id, &bytes);
            Ok(String::new())
        }
        Request::CapturePane {
            target,
            start,
            end,
            escape,
            join,
            preserve_trailing,
        } => state.capture_pane(
            target.as_deref(),
            start,
            end,
            escape,
            join,
            preserve_trailing,
        ),
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
            if reset {
                let (_, _, pane_id) = state.resolve_pane_target(target.as_deref())?;
                let pane_mode_clients = state
                    .clients
                    .iter()
                    .filter(|(_, client)| {
                        client
                            .panes_mode
                            .as_ref()
                            .is_some_and(|mode| mode.target_pane == pane_id)
                    })
                    .map(|(client_id, _)| *client_id)
                    .collect::<Vec<_>>();
                for client_id in pane_mode_clients {
                    state.exit_panes_mode(client_id);
                }
                if let Some(pane) = state.find_pane_mut(pane_id) {
                    pane.copy_mode = None;
                    pane.copy_source = None;
                    pane.panes_mode = false;
                }
                Ok(String::new())
            } else {
                // Mouse-triggered copy-mode commands are dispatched with the
                // event pane in `mouse_context`.  tmux's -M/-S flags use that
                // pane even when the command was installed as a custom
                // binding without an explicit target.
                let mouse_target = state
                    .mouse_context
                    .as_ref()
                    .filter(|_| mouse_start || scroll_to_mouse)
                    .map(|mouse| format!("%{}", mouse.pane_id));
                let target = mouse_target.as_deref().or(target.as_deref());
                state.enter_copy_mode(
                    target,
                    source.as_deref(),
                    exit_on_scroll,
                    hide_position,
                    kill_on_exit,
                    page,
                )?;
                let pane_id = state.resolve_pane_target(target)?.2;
                if let Some((mouse_row, mouse_col)) = state
                    .mouse_context
                    .as_ref()
                    .filter(|mouse| mouse.pane_id == pane_id)
                    .map(|mouse| (mouse.y, mouse.x))
                {
                    if mouse_start {
                        let history_limit = state.history_limit;
                        if let Some(pane) = state.find_pane_mut(pane_id) {
                            let mut source_parser = copy_source_parser(pane, history_limit);
                            if let Some(mode) = pane.copy_mode.as_mut() {
                                let parser = source_parser
                                    .as_mut()
                                    .map_or(&mut pane.parser, |parser| parser);
                                mode.mouse_position(parser, mouse_row, mouse_col, true, false);
                            }
                        }
                    }
                    if scroll_to_mouse {
                        let _ = state.execute_copy_action(
                            pane_id,
                            CopyAction::ScrollToMouse(Some(mouse_row)),
                            1,
                        );
                    }
                }
                if page_down {
                    let _ = state.execute_copy_action(pane_id, CopyAction::PageDown, 1);
                }
                Ok(String::new())
            }
        }
        Request::CopyModeCommand {
            target,
            action,
            repeat,
        } => state.execute_copy_action(
            state.resolve_pane_target(target.as_deref())?.2,
            parse_copy_action(&action)?,
            repeat as usize,
        ),
        Request::ChooseTree {
            target,
            filter,
            format,
            sort,
            reverse,
            hide_source,
            kill_on_exit,
        } => {
            let client_id = if let Some(target) = target.as_deref() {
                let session_index = state.resolve_pane_target(Some(target))?.0;
                let session_id = state.sessions[session_index].id;
                state
                    .clients
                    .iter()
                    .find(|(_, client)| client.session_id == session_id)
                    .map(|(client_id, _)| *client_id)
            } else {
                state.clients.keys().next().copied()
            };
            let client_id =
                client_id.ok_or_else(|| "choose-tree requires an attached client".to_owned())?;
            state.enter_tree_mode(
                client_id,
                target.as_deref(),
                format.as_deref(),
                filter.as_deref(),
                &sort,
                reverse,
                hide_source,
                kill_on_exit,
            )
        }
        Request::ChooseBuffer {
            target,
            filter,
            format,
            sort,
            reverse,
            kill_on_exit,
        } => {
            let client_id = state.client_for_mode_target(target.as_deref())?;
            state.enter_buffer_mode(
                client_id,
                target.as_deref(),
                format.as_deref(),
                filter.as_deref(),
                &sort,
                reverse,
                kill_on_exit,
            )
        }
        Request::ChooseClient {
            target,
            filter,
            format,
            kill_on_exit,
        } => {
            let client_id = state.client_for_mode_target(target.as_deref())?;
            state.enter_client_mode(
                client_id,
                target.as_deref(),
                format.as_deref(),
                filter.as_deref(),
                kill_on_exit,
            )
        }
        Request::DisplayPanes {
            target,
            source,
            no_zoom,
            no_mode,
            command,
            kill_on_exit,
        } => {
            let client_id = state.client_for_mode_target(target.as_deref())?;
            if no_mode {
                if !command.is_empty() {
                    let session_id = state.clients[&client_id].session_id;
                    state.execute_bound_commands(
                        client_id,
                        session_id,
                        ConfigBinding {
                            _repeat: false,
                            commands: vec![command],
                        },
                        shared,
                    )?;
                }
                Ok(String::new())
            } else {
                state.enter_panes_mode(
                    client_id,
                    target.as_deref(),
                    source.as_deref(),
                    no_zoom,
                    (!command.is_empty()).then_some(command),
                    kill_on_exit,
                )
            }
        }
        Request::DisplayMessage { target, format } => {
            state.display_message(target.as_deref(), &format)
        }
        Request::SetBuffer {
            name,
            append,
            data,
            rename,
        } => {
            if let Some(rename) = rename {
                state.rename_buffer(name.as_deref(), &rename)
            } else if data.is_empty() {
                if name.is_some() {
                    Err("no data specified".to_owned())
                } else {
                    Ok(String::new())
                }
            } else {
                state.store_buffer(name, data, append);
                Ok(String::new())
            }
        }
        Request::ShowBuffer { name } => state.show_buffer(name.as_deref()),
        Request::ListBuffers { format, filter } => {
            state.list_buffers(format.as_deref(), filter.as_deref())
        }
        Request::DeleteBuffer { name } => state.delete_buffer(name.as_deref()),
        Request::PasteBuffer {
            target,
            name,
            raw,
            bracketed,
            separator,
            delete,
        } => state.paste_buffer(
            target.as_deref(),
            name.as_deref(),
            raw,
            bracketed,
            separator.as_deref(),
            delete,
        ),
        Request::LoadBuffer { name, data } => state.load_buffer(name, data),
        Request::SaveBuffer { name, path, append } => {
            state.save_buffer(name.as_deref(), path.as_deref(), append)
        }
        Request::SetOption {
            target,
            scope,
            key,
            value,
            unset,
        } => state.set_option(target.as_deref(), scope, &key, &value, unset),
        Request::SetWindowOption { target, key, value } => {
            state.set_window_option(target.as_deref(), &key, &value)
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
        } => state.show_options(
            target.as_deref(),
            global,
            window,
            pane,
            value,
            all,
            quiet,
            key.as_deref(),
        ),
        Request::SetEnvironment {
            name,
            value,
            remove,
        } => state.set_environment(&name, value.as_deref(), remove),
        Request::ShowEnvironment { format, name } => {
            state.show_environment(format.as_deref(), name.as_deref())
        }
        Request::PipePane {
            target,
            command,
            toggle,
        } => state.pipe_pane(target.as_deref(), command.as_deref(), toggle),
        Request::KillServer => {
            state.shutdown = true;
            state.clients.clear();
            for session in &state.sessions {
                for window in &session.windows {
                    for pane in &window.panes {
                        pane.pty.kill();
                    }
                }
            }
            Ok(String::new())
        }
        Request::Attach { .. } => Err("attach request was handled separately".to_owned()),
    }
}

fn spawn_reader(shared: SharedState, pane_id: u64, pid: libc::pid_t, mut reader: std::fs::File) {
    thread::Builder::new()
        .name(format!("tm-pane-{pane_id}"))
        .spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(length) => {
                        if let Ok(mut state) = shared.lock() {
                            let mut updated = false;
                            let mut audible_bell = false;
                            let pipe = state
                                .pane_pipes
                                .get(&pane_id)
                                .map(|pipe| Arc::clone(&pipe.stdin));
                            for pane in state
                                .sessions
                                .iter_mut()
                                .flat_map(|session| &mut session.windows)
                                .flat_map(|window| &mut window.panes)
                                .filter(|pane| pane.pty.pid() == pid)
                            {
                                if let Some(pipe) = pipe.as_ref()
                                    && let Ok(mut stdin) = pipe.lock()
                                {
                                    let _ = stdin.write_all(&buffer[..length]);
                                }
                                retain_raw_output(pane, &buffer[..length]);
                                // OSC 2/7 metadata is emitted near the live
                                // cursor. Scanning the complete retained
                                // history for every PTY read made sustained
                                // output quadratic and blocked rendering on
                                // the global state lock. A bounded tail still
                                // covers sequences split across reads while
                                // keeping this path O(read size).
                                let metadata_start = pane
                                    .raw_output
                                    .len()
                                    .saturating_sub(TERMINAL_METADATA_SCAN_LIMIT);
                                let metadata = &pane.raw_output[metadata_start..];
                                if let Some(path) = terminal_path(metadata) {
                                    pane.current_path = Some(path);
                                }
                                let title = terminal_title(metadata);
                                pane.output_state
                                    .process(&mut pane.parser, &buffer[..length]);
                                audible_bell |= pane.output_state.take_audible_bell();
                                let refresh_live = pane.copy_source.is_none()
                                    && pane
                                        .copy_mode
                                        .as_ref()
                                        .is_some_and(CopyModeState::refresh_active);
                                if refresh_live {
                                    let raw_output = pane.raw_output.clone();
                                    if let Some(mode) = pane.copy_mode.as_mut() {
                                        mode.refresh_live(&mut pane.parser, &raw_output);
                                    }
                                }
                                if let Some(title) = title {
                                    pane.title = title;
                                }
                                updated = true;
                            }
                            if updated && audible_bell {
                                let global_monitor_bell =
                                    state.global_options.get("monitor-bell").cloned();
                                for session in &mut state.sessions {
                                    for window in &mut session.windows {
                                        if !window.panes.iter().any(|pane| pane.pty.pid() == pid) {
                                            continue;
                                        }
                                        let monitor_bell = window
                                            .options
                                            .get("monitor-bell")
                                            .or(global_monitor_bell.as_ref())
                                            .is_none_or(|value| {
                                                parse_on_off(value).unwrap_or(true)
                                            });
                                        if monitor_bell {
                                            window.bell_alert = true;
                                        }
                                    }
                                }
                            }
                            if updated {
                                state.mark_render_dirty();
                            }
                            if !updated {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            if let Ok(mut state) = shared.lock() {
                let exited = state
                    .sessions
                    .iter()
                    .flat_map(|session| &session.windows)
                    .flat_map(|window| &window.panes)
                    .filter(|pane| pane.pty.pid() == pid)
                    .map(|pane| pane.id)
                    .collect::<HashSet<_>>();
                if state.remain_on_exit {
                    for pane in state
                        .sessions
                        .iter_mut()
                        .flat_map(|session| &mut session.windows)
                        .flat_map(|window| &mut window.panes)
                        .filter(|pane| exited.contains(&pane.id))
                    {
                        pane.dead = true;
                    }
                } else {
                    state.remove_exited_panes(&exited);
                }
                state.mark_render_dirty();
            }
            Pty::reap(pid);
        })
        .ok();
}

const RAW_OUTPUT_LIMIT: usize = 1 << 20;
const TERMINAL_METADATA_SCAN_LIMIT: usize = 4096;

fn retain_raw_output(pane: &mut Pane, bytes: &[u8]) {
    pane.raw_output.extend_from_slice(bytes);
    if pane.raw_output.len() > RAW_OUTPUT_LIMIT {
        checkpoint_raw_output(pane);
    }
}

/// Replace an overgrown retained stream with a replayable terminal snapshot.
/// Dropping an arbitrary byte prefix can cut a UTF-8 scalar or control string,
/// making replay disagree with the live parser. `Screen::state_formatted`
/// produces a complete visible-state checkpoint, so future replay remains
/// well-formed even after the bounded history window rolls forward.
fn checkpoint_raw_output(pane: &mut Pane) {
    let screen = pane.parser.screen();
    let mut checkpoint = Vec::new();
    if screen.alternate_screen() {
        checkpoint.extend_from_slice(b"\x1b[?1049h");
    }
    checkpoint.extend_from_slice(&screen.state_formatted());
    checkpoint.extend_from_slice(&screen.cursor_state_formatted());
    pane.raw_output = checkpoint;
    pane.output_state = terminal::OutputState::default();
}

/// Extract the most recent OSC 2 title from the bytes retained for a pane.
/// `vt100` consumes the title internally but does not expose it in the 0.16
/// screen API, so keeping this tiny decoder at the PTY boundary lets format
/// variables observe title changes without introducing another terminal crate.
fn terminal_title(output: &[u8]) -> Option<String> {
    let marker = b"\x1b]2;";
    let start = output
        .windows(marker.len())
        .rposition(|window| window == marker)?;
    let value = &output[start + marker.len()..];
    let end = value
        .iter()
        .position(|byte| *byte == 0x07)
        .or_else(|| value.windows(2).position(|window| window == b"\x1b\\"))?;
    String::from_utf8(value[..end].to_vec()).ok()
}

/// Extract the most recent OSC 7 directory report. OSC 7 is the portable
/// shell/editor boundary for `pane_current_path`; unlike inspecting a child
/// process's cwd it works on both macOS and Linux and survives shell changes
/// after the pane was created.
fn terminal_path(output: &[u8]) -> Option<String> {
    let marker = b"\x1b]7;";
    let start = output
        .windows(marker.len())
        .rposition(|window| window == marker)?;
    let value = &output[start + marker.len()..];
    let end = value
        .iter()
        .position(|byte| *byte == 0x07)
        .or_else(|| value.windows(2).position(|window| window == b"\x1b\\"))?;
    let value = std::str::from_utf8(&value[..end]).ok()?;
    let path = if let Some(rest) = value.strip_prefix("file://") {
        if rest.starts_with('/') {
            rest.to_owned()
        } else {
            rest.split_once('/')
                .map(|(_, path)| format!("/{path}"))
                .unwrap_or_else(|| rest.to_owned())
        }
    } else {
        value.to_owned()
    };
    percent_decode_path(&path)
}

fn percent_decode_path(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut characters = value.bytes();
    while let Some(byte) = characters.next() {
        if byte == b'%' {
            let high = hex_digit(characters.next()?)?;
            let low = hex_digit(characters.next()?)?;
            bytes.push(high * 16 + low);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn unix_time() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) }
}

fn parse_split_size(value: &str, horizontal: bool, target: Rect) -> Option<u16> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent.parse::<u16>().ok()?.min(100);
        let length = if horizontal { target.cols } else { target.rows };
        return Some((u32::from(length).saturating_mul(u32::from(percent)) / 100) as u16);
    }
    value.parse::<u16>().ok()
}

fn render_format(format: &str, values: &[(&str, String)]) -> String {
    expand_format(format, values)
}

fn parse_format_loop(body: &str) -> Option<(char, String, String)> {
    let kind = body.chars().next()?;
    if !matches!(kind, 'S' | 'W' | 'P') {
        return None;
    }
    let rest = &body[kind.len_utf8()..];
    let (spec, value) = split_modifier_value(rest)?;
    Some((kind, unescape_format(spec), value.to_owned()))
}

fn render_format_with_options(
    format: &str,
    values: &[(&str, String)],
    options: &HashMap<String, String>,
) -> String {
    let mut values = values.to_vec();
    values.extend(
        options
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone())),
    );
    render_format(format, &values)
}

fn expand_format(format: &str, values: &[(&str, String)]) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < format.len() {
        if format[index..].starts_with("##") {
            output.push('#');
            index += 2;
            continue;
        }
        if format[index..].starts_with("#,")
            || format[index..].starts_with("#:")
            || format[index..].starts_with("#}")
        {
            output.push(format.as_bytes()[index + 1] as char);
            index += 2;
            continue;
        }
        if !format[index..].starts_with("#{") {
            let character = format[index..]
                .chars()
                .next()
                .expect("format index is on a character boundary");
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        let Some(end) = format_token_end(format, index + 2) else {
            output.push_str(&format[index..]);
            break;
        };
        let body = &format[index + 2..end];
        output.push_str(&expand_format_token(body, values));
        index = end + 1;
    }
    output
}

fn format_token_end(format: &str, start: usize) -> Option<usize> {
    let bytes = format.as_bytes();
    let mut depth = 1;
    let mut index = start;
    while index < bytes.len() {
        if bytes[index] == b'#'
            && index + 1 < bytes.len()
            && matches!(bytes[index + 1], b'}' | b',' | b':')
        {
            index += 2;
            continue;
        }
        if bytes[index] == b'#' && index + 1 < bytes.len() && bytes[index + 1] == b'{' {
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn expand_format_token(body: &str, values: &[(&str, String)]) -> String {
    if let Some(condition) = body.strip_prefix('?') {
        let parts = split_format_arguments(condition);
        let condition = parts.first().map_or("", String::as_str);
        let then_value = parts.get(1).map_or("", String::as_str);
        let else_value = parts.get(2).map_or("", String::as_str);
        return if format_condition(condition, values) {
            expand_format(then_value, values)
        } else {
            expand_format(else_value, values)
        };
    }
    if let Some(value) = body.strip_prefix("l:") {
        return unescape_format(value);
    }
    if body.starts_with("c:") || body.starts_with("c/f:") || body.starts_with("c/b:") {
        return format_colour_modifier(body, values).unwrap_or_default();
    }
    if body.starts_with('N')
        && let Some(value) = format_name_exists(body, values)
    {
        return value;
    }
    if body.starts_with('C')
        && let Some(value) = format_content_search(body, values)
    {
        return value;
    }
    if let Some(value) = body.strip_prefix("T:") {
        let value = evaluate_format_argument(value, values);
        if value.contains('%') {
            let now = unsafe { libc::time(std::ptr::null_mut()) };
            return format_strftime(now, &value);
        }
        return value;
    }
    for (operator, compare) in [("==:", true), ("!=:", false)] {
        if let Some(arguments) = body.strip_prefix(operator) {
            let parts = split_format_arguments(arguments);
            let (Some(left), Some(right)) = (parts.first(), parts.get(1)) else {
                return String::new();
            };
            let left = evaluate_format_argument(left, values);
            let right = evaluate_format_argument(right, values);
            return if (left == right) == compare {
                "1".to_owned()
            } else {
                "0".to_owned()
            };
        }
    }
    for (operator, compare) in [("<=:", 2), (">=:", 3), ("<:", -1), (">:", 1)] {
        if let Some(arguments) = body.strip_prefix(operator) {
            let parts = split_format_arguments(arguments);
            let (Some(left), Some(right)) = (parts.first(), parts.get(1)) else {
                return String::new();
            };
            let left = evaluate_format_argument(left, values);
            let right = evaluate_format_argument(right, values);
            let result = compare_format_values(&left, &right, compare);
            return result
                .then_some("1".to_owned())
                .unwrap_or_else(|| "0".to_owned());
        }
    }
    if body.starts_with("e|") {
        return format_arithmetic(body, values).unwrap_or_default();
    }
    if let Some(value) = body.strip_prefix("E:") {
        let expanded = evaluate_format_argument(value, values);
        if expanded == format!("#{{E:{value}}}") {
            return String::new();
        }
        return expand_format(&expanded, values);
    }
    if body.starts_with("t:") || body.starts_with("t/") {
        return format_time_modifier(body, values).unwrap_or_default();
    }
    if body.starts_with('s')
        && let Some(value) = format_substitute(body, values)
    {
        return value;
    }
    if let Some(value) = body.strip_prefix("n:") {
        return evaluate_format_argument(value, values).len().to_string();
    }
    if let Some(value) = body.strip_prefix("w:") {
        return format_display_width(&evaluate_format_argument(value, values)).to_string();
    }
    if let Some(value) = body.strip_prefix("b:") {
        return format_basename(&evaluate_format_argument(value, values));
    }
    if let Some(value) = body.strip_prefix("d:") {
        return format_dirname(&evaluate_format_argument(value, values));
    }
    if let Some(value) = body.strip_prefix("a:") {
        return evaluate_format_argument(value, values)
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .filter(|character| character.is_ascii())
            .map_or_else(String::new, |character| character.to_string());
    }
    // Variable names beginning with a modifier letter (notably the mouse
    // variables) must win over the modifier parser when they are exact
    // entries in the current format context.
    if let Some((_, value)) = values.iter().find(|(name, _)| *name == body) {
        return value.clone();
    }
    if let Some(value) = body.strip_prefix("R:") {
        let parts = split_format_arguments(value);
        if parts.len() != 2 {
            return String::new();
        }
        return format_repeat(
            &evaluate_format_argument(&parts[0], values),
            &evaluate_format_argument(&parts[1], values),
        );
    }
    if let Some((width, value)) = modifier_parts(body, "p") {
        return format_padded(&evaluate_format_argument(&value, values), &width);
    }
    if let Some((spec, value)) = modifier_parts(body, "=") {
        let (width, marker) = if let Some(spec) = spec.strip_prefix('/') {
            let Some(marker_start) = spec.find('/') else {
                return evaluate_format_argument(&value, values);
            };
            (
                &spec[..marker_start],
                unescape_format(&spec[marker_start + 1..]),
            )
        } else {
            (spec.as_str(), String::new())
        };
        return format_truncated(
            &evaluate_format_argument(&value, values),
            width,
            &marker,
            values,
        );
    }
    if let Some((mode, value)) = modifier_parts(body, "q") {
        return format_shell_quote(
            &evaluate_format_argument(&value, values),
            mode.strip_prefix('/').unwrap_or(&mode),
        );
    }
    if let Some(arguments) = body.strip_prefix("m") {
        let Some((spec, value)) =
            split_modifier_value(arguments.strip_prefix('/').unwrap_or(arguments))
        else {
            return String::new();
        };
        let parts = split_format_arguments(value);
        if parts.len() != 2 {
            return String::new();
        }
        let pattern = evaluate_format_argument(&parts[0], values);
        let text = evaluate_format_argument(&parts[1], values);
        let spec = unescape_format(spec);
        let ignore_case = spec.contains('i');
        if spec.contains('z') || spec.contains('p') {
            let text_chars = text.chars().collect::<Vec<_>>();
            let mut positions = Vec::new();
            let mut cursor = 0;
            for wanted in pattern.chars() {
                let Some(relative) = text_chars[cursor..].iter().position(|character| {
                    if ignore_case {
                        character.eq_ignore_ascii_case(&wanted)
                    } else {
                        *character == wanted
                    }
                }) else {
                    positions.clear();
                    break;
                };
                let position = cursor + relative;
                positions.push(position);
                cursor = position + 1;
            }
            if spec.contains('p') {
                return positions
                    .into_iter()
                    .map(|position| position.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
            }
            return if !pattern.is_empty() && !positions.is_empty() {
                "1".to_owned()
            } else {
                "0".to_owned()
            };
        }
        let matched = if spec.contains('r') {
            format_regex_find(&pattern, &text, ignore_case).is_some()
        } else {
            format_glob_match(&pattern, &text, ignore_case)
        };
        return if matched { "1" } else { "0" }.to_owned();
    }
    if let Some(arguments) = body.strip_prefix("||:") {
        return split_format_arguments(arguments)
            .iter()
            .any(|value| format_condition(value, values))
            .then_some("1".to_owned())
            .unwrap_or_else(|| "0".to_owned());
    }
    if let Some(arguments) = body.strip_prefix("&&:") {
        return split_format_arguments(arguments)
            .iter()
            .all(|value| format_condition(value, values))
            .then_some("1".to_owned())
            .unwrap_or_else(|| "0".to_owned());
    }
    if let Some(value) = body.strip_prefix("!:") {
        return (!format_condition(value, values))
            .then_some("1".to_owned())
            .unwrap_or_else(|| "0".to_owned());
    }
    if let Some(value) = body.strip_prefix("!!:") {
        return format_condition(value, values)
            .then_some("1".to_owned())
            .unwrap_or_else(|| "0".to_owned());
    }
    values
        .iter()
        .find(|(name, _)| *name == body)
        .map_or_else(String::new, |(_, value)| value.clone())
}

fn split_format_arguments(value: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut start = 0;
    let mut depth: usize = 0;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#'
            && index + 1 < bytes.len()
            && matches!(bytes[index + 1], b',' | b':' | b'}')
        {
            index += 2;
            continue;
        }
        if bytes[index] == b'#' && index + 1 < bytes.len() && bytes[index + 1] == b'{' {
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'}' {
            depth = depth.saturating_sub(1);
        } else if bytes[index] == b',' && depth == 0 {
            output.push(value[start..index].to_owned());
            start = index + 1;
        }
        index += 1;
    }
    output.push(value[start..].to_owned());
    output
}

fn unescape_format(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#'
            && index + 1 < bytes.len()
            && matches!(bytes[index + 1], b'#' | b',' | b':' | b'}')
        {
            output.push(bytes[index + 1] as char);
            index += 2;
            continue;
        }
        let character = value[index..]
            .chars()
            .next()
            .expect("format index is on a character boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn evaluate_format_argument(value: &str, values: &[(&str, String)]) -> String {
    let expanded = expand_format(value, values);
    if expanded != value || value.contains("#{") {
        expanded
    } else {
        values
            .iter()
            .find(|(name, _)| *name == value)
            .map_or_else(|| unescape_format(value), |(_, value)| value.clone())
    }
}

fn compare_format_values(left: &str, right: &str, operator: i32) -> bool {
    if let (Ok(left), Ok(right)) = (left.parse::<i64>(), right.parse::<i64>()) {
        return match operator {
            -1 => left < right,
            1 => left > right,
            2 => left <= right,
            3 => left >= right,
            _ => false,
        };
    }
    match operator {
        -1 => left < right,
        1 => left > right,
        2 => left <= right,
        3 => left >= right,
        _ => false,
    }
}

fn split_modifier_value(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'#' && index + 1 < bytes.len() {
            if matches!(bytes[index + 1], b'#' | b',' | b':' | b'}') {
                index += 2;
                continue;
            }
            if bytes[index + 1] == b'{' {
                depth += 1;
                index += 2;
                continue;
            }
        }
        if bytes[index] == b'}' && depth > 0 {
            depth -= 1;
        } else if bytes[index] == b':' && depth == 0 {
            return Some((&value[..index], &value[index + 1..]));
        }
        index += 1;
    }
    None
}

fn modifier_parts(body: &str, prefix: &str) -> Option<(String, String)> {
    let rest = body.strip_prefix(prefix)?;
    let (modifier, value) = split_modifier_value(rest)?;
    Some((unescape_format(modifier), value.to_owned()))
}

fn format_char_width(character: char, current: usize) -> usize {
    if character == '\t' {
        8 - current % 8
    } else if matches!(
        character as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
    ) {
        2
    } else if character.is_control() {
        0
    } else {
        1
    }
}

fn format_display_width(value: &str) -> usize {
    value.chars().fold(0, |width, character| {
        width.saturating_add(format_char_width(character, width))
    })
}

fn take_display_width(value: &str, limit: usize, from_end: bool) -> String {
    if from_end {
        let mut output = String::new();
        let mut width = 0;
        for character in value.chars().rev() {
            let character_width = format_char_width(character, width);
            if width + character_width > limit {
                break;
            }
            width += character_width;
            output.insert(0, character);
        }
        output
    } else {
        let mut output = String::new();
        let mut width = 0;
        for character in value.chars() {
            let character_width = format_char_width(character, width);
            if width + character_width > limit {
                break;
            }
            width += character_width;
            output.push(character);
        }
        output
    }
}

fn format_basename(value: &str) -> String {
    value
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(value)
        .to_owned()
}

fn format_dirname(value: &str) -> String {
    if let Some(index) = value.rfind('/') {
        if index == 0 {
            "/".to_owned()
        } else {
            value[..index].to_owned()
        }
    } else {
        ".".to_owned()
    }
}

fn format_shell_quote(value: &str, mode: &str) -> String {
    match mode {
        "s" => format!("'{}'", value.replace('\'', "'\\''")),
        "a" => format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
        "e" | "h" => value.replace('#', "##"),
        _ => {
            let mut output = String::new();
            for character in value.chars() {
                if matches!(
                    character,
                    ' ' | '\t'
                        | '\n'
                        | '\\'
                        | '$'
                        | '`'
                        | '"'
                        | '\''
                        | ';'
                        | '&'
                        | '|'
                        | '<'
                        | '>'
                        | '('
                        | ')'
                        | '*'
                        | '?'
                        | '['
                        | ']'
                        | '!'
                        | '#'
                ) {
                    output.push('\\');
                }
                output.push(character);
            }
            output
        }
    }
}

fn format_glob_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    fn matches(pattern: &[char], text: &[char], ignore_case: bool) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        if pattern[0] == '*' {
            return matches(&pattern[1..], text, ignore_case)
                || (!text.is_empty() && matches(pattern, &text[1..], ignore_case));
        }
        if text.is_empty() {
            return false;
        }
        let equal = |left: char, right: char| {
            if ignore_case {
                left.eq_ignore_ascii_case(&right)
            } else {
                left == right
            }
        };
        (pattern[0] == '?' || equal(pattern[0], text[0]))
            && matches(&pattern[1..], &text[1..], ignore_case)
    }
    matches(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
        ignore_case,
    )
}

#[derive(Clone)]
enum FormatRegexAtom {
    Literal(char),
    Any,
    Class(Vec<(char, char)>, bool),
    Group(Vec<FormatRegexNode>, usize),
    Alternation(Vec<Vec<FormatRegexNode>>),
    Backref(usize),
    Begin,
    End,
}

#[derive(Clone, Copy)]
enum FormatRegexQuantifier {
    Once,
    Optional,
    ZeroOrMore,
    OneOrMore,
    Range(usize, Option<usize>),
}

#[derive(Clone)]
struct FormatRegexNode {
    atom: FormatRegexAtom,
    quantifier: FormatRegexQuantifier,
}

fn parse_format_regex(pattern: &str) -> Option<Vec<FormatRegexNode>> {
    fn parse_count(characters: &[char], index: &mut usize) -> Option<usize> {
        let start = *index;
        let mut count = 0usize;
        while let Some(character) = characters.get(*index).copied() {
            let Some(digit) = character.to_digit(10) else {
                break;
            };
            count = count.checked_mul(10)?.checked_add(digit as usize)?;
            *index += 1;
        }
        (*index != start).then_some(count)
    }

    fn parse_sequence(
        characters: &[char],
        index: &mut usize,
        groups: &mut usize,
    ) -> Option<Vec<FormatRegexNode>> {
        let mut nodes = Vec::new();
        while *index < characters.len() {
            if matches!(characters[*index], ')' | '|') {
                break;
            }
            let atom = match characters[*index] {
                '^' => {
                    *index += 1;
                    FormatRegexAtom::Begin
                }
                '$' => {
                    *index += 1;
                    FormatRegexAtom::End
                }
                '.' => {
                    *index += 1;
                    FormatRegexAtom::Any
                }
                '(' => {
                    *index += 1;
                    *groups += 1;
                    let group = *groups;
                    let alternatives = parse_alternatives(characters, index, groups, true)?;
                    let nodes = if alternatives.len() == 1 {
                        alternatives.into_iter().next()?
                    } else {
                        vec![FormatRegexNode {
                            atom: FormatRegexAtom::Alternation(alternatives),
                            quantifier: FormatRegexQuantifier::Once,
                        }]
                    };
                    FormatRegexAtom::Group(nodes, group)
                }
                '[' => {
                    *index += 1;
                    let negated = characters.get(*index) == Some(&'^');
                    if negated {
                        *index += 1;
                    }
                    let mut ranges = Vec::new();
                    let mut closed = false;
                    while *index < characters.len() {
                        if characters[*index] == ']' {
                            *index += 1;
                            closed = true;
                            break;
                        }
                        let first = if characters[*index] == '\\' {
                            *index += 1;
                            *characters.get(*index)?
                        } else {
                            characters[*index]
                        };
                        *index += 1;
                        if characters.get(*index) == Some(&'-')
                            && *index + 1 < characters.len()
                            && characters[*index + 1] != ']'
                        {
                            *index += 1;
                            let last = if characters[*index] == '\\' {
                                *index += 1;
                                *characters.get(*index)?
                            } else {
                                characters[*index]
                            };
                            *index += 1;
                            ranges.push((first, last));
                        } else {
                            ranges.push((first, first));
                        }
                    }
                    if !closed {
                        return None;
                    }
                    FormatRegexAtom::Class(ranges, negated)
                }
                '\\' => {
                    *index += 1;
                    let escaped = *characters.get(*index)?;
                    *index += 1;
                    if let Some(digit) = escaped.to_digit(10) {
                        FormatRegexAtom::Backref(digit as usize)
                    } else if escaped == '$' {
                        FormatRegexAtom::End
                    } else if escaped == '^' {
                        FormatRegexAtom::Begin
                    } else {
                        FormatRegexAtom::Literal(escaped)
                    }
                }
                character => {
                    *index += 1;
                    FormatRegexAtom::Literal(character)
                }
            };
            let quantifier = match characters.get(*index) {
                Some('*') => {
                    *index += 1;
                    FormatRegexQuantifier::ZeroOrMore
                }
                Some('+') => {
                    *index += 1;
                    FormatRegexQuantifier::OneOrMore
                }
                Some('?') => {
                    *index += 1;
                    FormatRegexQuantifier::Optional
                }
                Some('{') => {
                    *index += 1;
                    let minimum = parse_count(characters, index)?;
                    let maximum = match characters.get(*index) {
                        Some('}') => {
                            *index += 1;
                            Some(minimum)
                        }
                        Some(',') => {
                            *index += 1;
                            let maximum = if characters.get(*index) == Some(&'}') {
                                None
                            } else {
                                Some(parse_count(characters, index)?)
                            };
                            if characters.get(*index) != Some(&'}') {
                                return None;
                            }
                            *index += 1;
                            maximum
                        }
                        _ => return None,
                    };
                    if maximum.is_some_and(|maximum| maximum < minimum) {
                        return None;
                    }
                    FormatRegexQuantifier::Range(minimum, maximum)
                }
                _ => FormatRegexQuantifier::Once,
            };
            nodes.push(FormatRegexNode { atom, quantifier });
        }
        Some(nodes)
    }

    fn parse_alternatives(
        characters: &[char],
        index: &mut usize,
        groups: &mut usize,
        nested: bool,
    ) -> Option<Vec<Vec<FormatRegexNode>>> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(parse_sequence(characters, index, groups)?);
            match characters.get(*index) {
                Some('|') => *index += 1,
                Some(')') if nested => {
                    *index += 1;
                    return Some(alternatives);
                }
                None if !nested => return Some(alternatives),
                _ => return None,
            }
        }
    }

    let characters = pattern.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut groups = 0;
    let alternatives = parse_alternatives(&characters, &mut index, &mut groups, false)?;
    if alternatives.len() == 1 {
        alternatives.into_iter().next()
    } else {
        Some(vec![FormatRegexNode {
            atom: FormatRegexAtom::Alternation(alternatives),
            quantifier: FormatRegexQuantifier::Once,
        }])
    }
}

type FormatRegexCaptures = Vec<Option<(usize, usize)>>;

fn format_regex_atom_matches(
    atom: &FormatRegexAtom,
    characters: &[char],
    position: usize,
    captures: &FormatRegexCaptures,
    ignore_case: bool,
) -> Vec<(usize, FormatRegexCaptures)> {
    match atom {
        FormatRegexAtom::Literal(expected) => {
            let Some(actual) = characters.get(position) else {
                return Vec::new();
            };
            let equal = if ignore_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == expected
            };
            equal
                .then(|| (position + 1, captures.clone()))
                .into_iter()
                .collect()
        }
        FormatRegexAtom::Any => characters
            .get(position)
            .map(|_| (position + 1, captures.clone()))
            .into_iter()
            .collect(),
        FormatRegexAtom::Class(ranges, negated) => {
            let Some(actual) = characters.get(position) else {
                return Vec::new();
            };
            let matched = ranges.iter().any(|(first, last)| {
                if ignore_case {
                    actual.to_ascii_lowercase() >= first.to_ascii_lowercase()
                        && actual.to_ascii_lowercase() <= last.to_ascii_lowercase()
                } else {
                    actual >= first && actual <= last
                }
            });
            (matched != *negated)
                .then(|| (position + 1, captures.clone()))
                .into_iter()
                .collect()
        }
        FormatRegexAtom::Begin => (position == 0)
            .then(|| (position, captures.clone()))
            .into_iter()
            .collect(),
        FormatRegexAtom::End => (position == characters.len())
            .then(|| (position, captures.clone()))
            .into_iter()
            .collect(),
        FormatRegexAtom::Backref(group) => {
            let Some((start, end)) = captures.get(*group).and_then(Option::as_ref).copied() else {
                return Vec::new();
            };
            let length = end.saturating_sub(start);
            if position + length > characters.len() {
                return Vec::new();
            }
            let equal = characters[start..end]
                .iter()
                .zip(&characters[position..position + length])
                .all(|(left, right)| {
                    if ignore_case {
                        left.eq_ignore_ascii_case(right)
                    } else {
                        left == right
                    }
                });
            equal
                .then(|| (position + length, captures.clone()))
                .into_iter()
                .collect()
        }
        FormatRegexAtom::Group(nodes, group) => {
            format_regex_match_all(nodes, characters, 0, position, captures, ignore_case)
                .into_iter()
                .map(|(end, mut output)| {
                    if output.len() <= *group {
                        output.resize(*group + 1, None);
                    }
                    output[*group] = Some((position, end));
                    (end, output)
                })
                .collect()
        }
        FormatRegexAtom::Alternation(alternatives) => alternatives
            .iter()
            .flat_map(|alternative| {
                format_regex_match_all(alternative, characters, 0, position, captures, ignore_case)
            })
            .collect(),
    }
}

fn format_regex_match_all(
    nodes: &[FormatRegexNode],
    characters: &[char],
    index: usize,
    position: usize,
    captures: &FormatRegexCaptures,
    ignore_case: bool,
) -> Vec<(usize, FormatRegexCaptures)> {
    if index == nodes.len() {
        return vec![(position, captures.clone())];
    }
    let node = &nodes[index];
    match node.quantifier {
        FormatRegexQuantifier::Once => {
            format_regex_atom_matches(&node.atom, characters, position, captures, ignore_case)
                .into_iter()
                .flat_map(|(position, captures)| {
                    format_regex_match_all(
                        nodes,
                        characters,
                        index + 1,
                        position,
                        &captures,
                        ignore_case,
                    )
                })
                .collect()
        }
        FormatRegexQuantifier::Optional => {
            let mut results = format_regex_match_all(
                nodes,
                characters,
                index + 1,
                position,
                captures,
                ignore_case,
            );
            for (position, captures) in
                format_regex_atom_matches(&node.atom, characters, position, captures, ignore_case)
            {
                results.extend(format_regex_match_all(
                    nodes,
                    characters,
                    index + 1,
                    position,
                    &captures,
                    ignore_case,
                ));
            }
            results
        }
        FormatRegexQuantifier::ZeroOrMore
        | FormatRegexQuantifier::OneOrMore
        | FormatRegexQuantifier::Range(_, _) => {
            let (minimum, maximum) = match node.quantifier {
                FormatRegexQuantifier::ZeroOrMore => (0, None),
                FormatRegexQuantifier::OneOrMore => (1, None),
                FormatRegexQuantifier::Range(minimum, maximum) => (minimum, maximum),
                _ => unreachable!(),
            };
            let mut levels = vec![vec![(position, captures.clone())]];
            loop {
                if maximum.is_some_and(|maximum| levels.len().saturating_sub(1) >= maximum) {
                    break;
                }
                let next = levels
                    .last()
                    .into_iter()
                    .flatten()
                    .flat_map(|(last_position, last_captures)| {
                        format_regex_atom_matches(
                            &node.atom,
                            characters,
                            *last_position,
                            last_captures,
                            ignore_case,
                        )
                        .into_iter()
                        .filter(|(next_position, _)| *next_position != *last_position)
                    })
                    .collect::<Vec<_>>();
                if next.is_empty() {
                    break;
                }
                levels.push(next);
            }
            levels
                .into_iter()
                .enumerate()
                .rev()
                .filter(|(count, _)| *count >= minimum)
                .flat_map(|(_, states)| {
                    states.into_iter().flat_map(|(position, captures)| {
                        format_regex_match_all(
                            nodes,
                            characters,
                            index + 1,
                            position,
                            &captures,
                            ignore_case,
                        )
                    })
                })
                .collect()
        }
    }
}

fn format_regex_match(
    nodes: &[FormatRegexNode],
    characters: &[char],
    index: usize,
    position: usize,
    captures: &FormatRegexCaptures,
    ignore_case: bool,
) -> Option<(usize, FormatRegexCaptures)> {
    format_regex_match_all(nodes, characters, index, position, captures, ignore_case)
        .into_iter()
        .next()
}

fn format_regex_find(
    pattern: &str,
    text: &str,
    ignore_case: bool,
) -> Option<(usize, usize, FormatRegexCaptures)> {
    let nodes = parse_format_regex(pattern)?;
    let characters = text.chars().collect::<Vec<_>>();
    let anchored = matches!(
        nodes.first().map(|node| &node.atom),
        Some(FormatRegexAtom::Begin)
    );
    let starts = if anchored {
        vec![0]
    } else {
        (0..=characters.len()).collect::<Vec<_>>()
    };
    let groups = nodes
        .iter()
        .filter_map(|node| match node.atom {
            FormatRegexAtom::Group(_, group) => Some(group),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    for start in starts {
        let captures = vec![None; groups + 1];
        if let Some((end, captures)) =
            format_regex_match(&nodes, &characters, 0, start, &captures, ignore_case)
        {
            return Some((start, end, captures));
        }
    }
    None
}

/// Copy mode and format substitution share the same dependency-free regular
/// expression grammar. Copy mode only needs the byte bounds of the first
/// match; capture groups remain internal to format replacement.
pub(crate) fn copy_mode_regex_find(
    pattern: &str,
    text: &str,
    ignore_case: bool,
) -> Option<(usize, usize)> {
    let (start, end, _) = format_regex_find(pattern, text, ignore_case)?;
    let byte_offset = |character: usize| {
        text.char_indices()
            .nth(character)
            .map_or(text.len(), |(offset, _)| offset)
    };
    Some((byte_offset(start), byte_offset(end)))
}

fn format_regex_replacement(
    replacement: &str,
    characters: &[char],
    captures: &FormatRegexCaptures,
) -> String {
    let mut output = String::new();
    let replacement = replacement.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < replacement.len() {
        if replacement[index] == '\\'
            && index + 1 < replacement.len()
            && replacement[index + 1].is_ascii_digit()
        {
            let group = replacement[index + 1].to_digit(10).unwrap_or(0) as usize;
            if let Some(Some((start, end))) = captures.get(group) {
                output.extend(characters[*start..*end].iter());
            }
            index += 2;
        } else if replacement[index] == '\\' && index + 1 < replacement.len() {
            output.push(replacement[index + 1]);
            index += 2;
        } else {
            output.push(replacement[index]);
            index += 1;
        }
    }
    output
}

fn format_regex_replace(
    source: &str,
    pattern: &str,
    replacement: &str,
    ignore_case: bool,
) -> String {
    let source_characters = source.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut cursor = 0;
    while cursor <= source_characters.len() {
        let remaining = source_characters[cursor..].iter().collect::<String>();
        let Some((start, end, captures)) = format_regex_find(pattern, &remaining, ignore_case)
        else {
            output.extend(source_characters[cursor..].iter());
            break;
        };
        output.extend(source_characters[cursor..cursor + start].iter());
        output.push_str(&format_regex_replacement(
            replacement,
            &source_characters[cursor..],
            &captures,
        ));
        if end == start {
            let position = cursor + start;
            if let Some(character) = source_characters.get(position) {
                output.push(*character);
                cursor = position + 1;
            } else {
                cursor = source_characters.len() + 1;
            }
        } else {
            cursor += end;
        }
    }
    output
}

fn format_strftime(timestamp: libc::time_t, format: &str) -> String {
    let Ok(format) = CString::new(format) else {
        return String::new();
    };
    let mut broken_down = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&timestamp, &mut broken_down) }.is_null() {
        return String::new();
    }
    let mut buffer = [0u8; 256];
    let length = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            format.as_ptr().cast(),
            &broken_down,
        )
    };
    String::from_utf8_lossy(&buffer[..length]).into_owned()
}

fn format_timestamp(value: &str) -> Option<libc::time_t> {
    value.trim().parse::<libc::time_t>().ok()
}

fn format_relative_time(timestamp: libc::time_t, now: libc::time_t) -> String {
    let age = now.saturating_sub(timestamp);
    if age < 0 {
        return String::new();
    }
    let (amount, unit) = if age < 60 {
        (age, "second")
    } else if age < 3600 {
        (age / 60, "minute")
    } else if age < 86_400 {
        (age / 3600, "hour")
    } else if age < 2_592_000 {
        (age / 86_400, "day")
    } else if age < 31_536_000 {
        (age / 2_592_000, "month")
    } else {
        (age / 31_536_000, "year")
    };
    format!("{amount} {unit}{} ago", if amount == 1 { "" } else { "s" })
}

fn format_time_modifier(body: &str, values: &[(&str, String)]) -> Option<String> {
    if let Some(value) = body.strip_prefix("t/f/") {
        let (format, value) = split_modifier_value(value)?;
        let timestamp = format_timestamp(&evaluate_format_argument(value, values))?;
        return Some(format_strftime(timestamp, &unescape_format(format)));
    }
    let (mode, value) = split_modifier_value(body.strip_prefix("t/").unwrap_or(body))?;
    let value = evaluate_format_argument(value, values);
    let timestamp = format_timestamp(&value)?;
    if body.starts_with("t/p:") {
        return Some(format_strftime(timestamp, "%b%y"));
    }
    if body.starts_with("t/r:") {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        return Some(format_relative_time(timestamp, now));
    }
    if body.starts_with("t/d:") {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        return Some(now.saturating_sub(timestamp).to_string());
    }
    if body.starts_with("t:") {
        let format = "%a %b %e %H:%M:%S %Y";
        return Some(format_strftime(timestamp, format));
    }
    let _ = mode;
    None
}

fn format_colour(value: &str) -> Option<String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(value.to_ascii_lowercase());
    }
    let palette = [
        "000000", "800000", "008000", "808000", "000080", "800080", "008080", "c0c0c0", "808080",
        "ff0000", "00ff00", "ffff00", "0000ff", "ff00ff", "00ffff", "ffffff",
    ];
    if let Some(index) = value
        .strip_prefix("colour")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return palette.get(index).map(|value| (*value).to_owned());
    }
    [
        ("black", "000000"),
        ("red", "800000"),
        ("green", "008000"),
        ("yellow", "808000"),
        ("blue", "000080"),
        ("magenta", "800080"),
        ("cyan", "008080"),
        ("white", "c0c0c0"),
        ("default", "000000"),
    ]
    .iter()
    .find(|(name, _)| *name == value)
    .map(|(_, value)| (*value).to_owned())
}

fn format_colour_modifier(body: &str, values: &[(&str, String)]) -> Option<String> {
    let (mode, value) = if let Some(value) = body.strip_prefix("c/f:") {
        ("f", value)
    } else if let Some(value) = body.strip_prefix("c/b:") {
        ("b", value)
    } else {
        ("", body.strip_prefix("c:")?)
    };
    let value = evaluate_format_argument(value, values);
    if mode.is_empty() {
        return format_colour(&value);
    }
    if value == "none" {
        return Some("\x1b[0m".to_owned());
    }
    if let Some(index) = value
        .strip_prefix("colour")
        .and_then(|value| value.parse::<u16>().ok())
    {
        return Some(format!(
            "\x1b[{};5;{}m",
            if mode == "f" { 38 } else { 48 },
            index
        ));
    }
    let code = match value.as_str() {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" => 5,
        "cyan" => 6,
        "white" => 7,
        _ => return None,
    };
    Some(format!(
        "\x1b[{}m",
        if mode == "f" { 30 + code } else { 40 + code }
    ))
}

fn format_repeat(value: &str, count: &str) -> String {
    let Ok(count) = count.parse::<usize>() else {
        return String::new();
    };
    if count == 0 || count > 1_000_000 {
        return String::new();
    }
    value.repeat(count)
}

fn format_numeric(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn format_arithmetic(body: &str, values: &[(&str, String)]) -> Option<String> {
    let (header, arguments) = split_modifier_value(body)?;
    let parts = header.split('|').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 4 || parts[0] != "e" {
        return None;
    }
    let operator = parts[1];
    let floating = parts.get(2).copied() == Some("f");
    let precision = if floating {
        if parts.len() == 4 {
            parts[3].parse::<usize>().ok()?
        } else {
            2
        }
    } else if parts.len() != 2 && !(parts.len() == 3 && parts[2].is_empty()) {
        return None;
    } else {
        0
    };
    let arguments = split_format_arguments(arguments);
    if arguments.len() != 2 {
        return None;
    }
    if floating {
        let left = evaluate_format_argument(&arguments[0], values)
            .parse::<f64>()
            .ok()?;
        let right = evaluate_format_argument(&arguments[1], values)
            .parse::<f64>()
            .ok()?;
        let result = match operator {
            "+" => left + right,
            "-" => left - right,
            "*" => left * right,
            "/" if right != 0.0 => left / right,
            "==" => {
                return Some(
                    (left == right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            "!=" => {
                return Some(
                    (left != right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            "<" => {
                return Some(
                    (left < right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            ">" => {
                return Some(
                    (left > right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            "<=" => {
                return Some(
                    (left <= right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            ">=" => {
                return Some(
                    (left >= right)
                        .then_some("1".to_owned())
                        .unwrap_or_else(|| "0".to_owned()),
                );
            }
            _ => return None,
        };
        return Some(format!("{result:.precision$}"));
    }
    let left = format_numeric(&evaluate_format_argument(&arguments[0], values))?;
    let right = format_numeric(&evaluate_format_argument(&arguments[1], values))?;
    let result = match operator {
        "+" => left.checked_add(right)?,
        "-" => left.checked_sub(right)?,
        "*" => left.checked_mul(right)?,
        "/" => left.checked_div(right)?,
        "m" | "%" => left.checked_rem(right)?,
        "==" => {
            return Some(
                (left == right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        "!=" => {
            return Some(
                (left != right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        "<" => {
            return Some(
                (left < right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        ">" => {
            return Some(
                (left > right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        "<=" => {
            return Some(
                (left <= right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        ">=" => {
            return Some(
                (left >= right)
                    .then_some("1".to_owned())
                    .unwrap_or_else(|| "0".to_owned()),
            );
        }
        _ => return None,
    };
    Some(result.to_string())
}

fn format_substitute(body: &str, values: &[(&str, String)]) -> Option<String> {
    let rest = body.strip_prefix("s")?;
    let delimiter = rest.chars().next()?;
    let delimiter_len = delimiter.len_utf8();
    let rest = &rest[delimiter_len..];
    let pattern_end = rest.find(delimiter)?;
    let pattern = &rest[..pattern_end];
    let rest = &rest[pattern_end + delimiter_len..];
    let replacement_end = rest.find(delimiter)?;
    let replacement = &rest[..replacement_end];
    let flags_end = replacement_end + delimiter_len;
    let (flags, value) = split_modifier_value(&rest[flags_end..])?;
    let source = evaluate_format_argument(value, values);
    let pattern = unescape_format(pattern);
    let replacement = unescape_format(replacement);
    let ignore_case = flags.contains('i');
    if pattern.is_empty() {
        return Some(source);
    }
    Some(format_regex_replace(
        &source,
        &pattern,
        &replacement,
        ignore_case,
    ))
}

fn format_padded(value: &str, width: &str) -> String {
    let Ok(width) = width.parse::<i32>() else {
        return value.to_owned();
    };
    let width_abs = width.unsigned_abs() as usize;
    let padding = width_abs.saturating_sub(format_display_width(value));
    if width < 0 {
        format!("{}{}", " ".repeat(padding), value)
    } else {
        format!("{}{}", value, " ".repeat(padding))
    }
}

fn format_truncated(
    value: &str,
    width_spec: &str,
    marker: &str,
    values: &[(&str, String)],
) -> String {
    let Ok(width) = width_spec.parse::<i32>() else {
        return value.to_owned();
    };
    let marker = expand_format(marker, values);
    let limit = width.unsigned_abs() as usize;
    if format_display_width(value) <= limit {
        return value.to_owned();
    }
    let value = take_display_width(value, limit, width < 0);
    if width < 0 {
        format!("{marker}{value}")
    } else {
        format!("{value}{marker}")
    }
}

fn format_condition(condition: &str, values: &[(&str, String)]) -> bool {
    let expanded = evaluate_format_argument(condition, values);
    !expanded.is_empty() && expanded != "0" && expanded != "false"
}

/// Implement the name-existence modifier for the current render context.
///
/// `N/s` and `N/w` are evaluated against the session or window represented by
/// the format values. Keeping this in the format layer makes the modifier
/// available consistently to display, list, and loop renders without adding a
/// second command-specific parser.
fn format_name_exists(body: &str, values: &[(&str, String)]) -> Option<String> {
    let rest = body.strip_prefix('N')?;
    let (modifier, argument) = split_modifier_value(rest)?;
    let modifier = modifier.strip_prefix('/').unwrap_or(modifier);
    let argument = evaluate_format_argument(argument, values);
    let names = match modifier {
        "" | "w" => values
            .iter()
            .find(|(name, _)| *name == "__window_names")
            .map(|(_, value)| value.as_str())
            .or_else(|| {
                values
                    .iter()
                    .find(|(name, _)| *name == "window_name")
                    .map(|(_, value)| value.as_str())
            }),
        "s" => values
            .iter()
            .find(|(name, _)| *name == "__session_names")
            .map(|(_, value)| value.as_str())
            .or_else(|| {
                values
                    .iter()
                    .find(|(name, _)| *name == "session_name")
                    .map(|(_, value)| value.as_str())
            }),
        _ => None,
    };
    Some(
        if names.is_some_and(|names| names.split('\0').any(|name| name == argument)) {
            "1"
        } else {
            "0"
        }
        .to_owned(),
    )
}

/// Search pane rows for the `C` format modifier.
///
/// Replaying the retained PTY bytes into a private VT parser gives the format
/// engine a normalized history-and-screen view without mutating the live
/// parser's scroll position. The retained byte window is bounded at the PTY
/// boundary, so this remains finite for a long-lived pane.
fn format_content_search(body: &str, values: &[(&str, String)]) -> Option<String> {
    let rest = body.strip_prefix('C')?;
    let (modifier, pattern) = split_modifier_value(rest)?;
    let modifier = modifier.strip_prefix('/').unwrap_or(modifier);
    let pattern = unescape_format(&evaluate_format_argument(pattern, values));
    let content = values
        .iter()
        .find(|(name, _)| *name == "__pane_content")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    let ignore_case = modifier.contains('i');
    let regex = modifier.contains('r');
    let found = content.lines().position(|line| {
        if regex {
            format_regex_find(&pattern, line, ignore_case).is_some()
        } else if ignore_case {
            line.to_ascii_lowercase()
                .contains(&pattern.to_ascii_lowercase())
        } else {
            line.contains(&pattern)
        }
    });
    Some(found.map_or_else(|| "0".to_owned(), |line| (line + 1).to_string()))
}

fn pane_content_for_format(pane: &Pane) -> String {
    if pane.raw_output.is_empty() {
        return pane.parser.screen().contents();
    }
    let mut parser = Parser::new(pane.rect.rows.max(1), pane.rect.cols.max(1), 10_000);
    terminal::replay(&mut parser, &pane.raw_output);
    let (mut history, live) = history_rows(&mut parser);
    let floor = pane.history_floor.min(history.len());
    history.drain(..floor);
    history
        .into_iter()
        .chain(live)
        .collect::<Vec<_>>()
        .join("\n")
}

fn pane_filter(pane: &Pane, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if let Some(inner) = filter
        .strip_prefix("#{==:#{pane_id},")
        .and_then(|value| value.strip_suffix('}'))
    {
        return format!("%{}", pane.id) == inner;
    }
    true
}

fn renumber_session_windows(session: &mut Session) {
    let active_window_id = session
        .windows
        .iter()
        .find(|window| window.index == session.active_window)
        .map(|window| window.id);
    let last_window_id = session.last_window.and_then(|index| {
        session
            .windows
            .iter()
            .find(|window| window.index == index)
            .map(|window| window.id)
    });
    session.windows.sort_by_key(|window| window.index);
    for (index, window) in session.windows.iter_mut().enumerate() {
        window.index = session.base_index.saturating_add(index as u32);
    }
    session.active_window = active_window_id
        .and_then(|window_id| session.windows.iter().find(|window| window.id == window_id))
        .map(|window| window.index)
        .unwrap_or_else(|| {
            session
                .windows
                .first()
                .map_or(session.base_index, |window| window.index)
        });
    session.last_window = last_window_id
        .and_then(|window_id| session.windows.iter().find(|window| window.id == window_id))
        .map(|window| window.index);
    session.next_window_index = session
        .base_index
        .saturating_add(session.windows.len() as u32);
}

fn format_session_result(session: &Session, format: Option<&str>) -> String {
    format.map_or_else(
        || session.name.clone(),
        |format| {
            let window = session.windows.first();
            render_format(
                format,
                &[
                    ("session_id", format!("${}", session.id)),
                    ("session_name", session.name.clone()),
                    ("session_windows", session.windows.len().to_string()),
                    (
                        "window_id",
                        window.map_or_else(String::new, |window| format!("@{}", window.id)),
                    ),
                    (
                        "window_name",
                        window.map_or_else(String::new, |window| window.name.clone()),
                    ),
                    ("pid", std::process::id().to_string()),
                ],
            )
        },
    )
}

fn quote_option_value(value: &str) -> String {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_owned()
    }
}

fn shell_quote_argument(value: &str) -> String {
    if value.is_empty() {
        "''".to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn format_environment(format: Option<&str>, name: &str, value: &str) -> String {
    format.map_or_else(
        || format!("{name}={value}"),
        |format| {
            render_format(
                format,
                &[
                    ("environment_name", name.to_owned()),
                    ("environment_value", value.to_owned()),
                ],
            )
        },
    )
}

fn format_pane(
    session: &Session,
    window: &Window,
    pane: &Pane,
    marked: bool,
    marked_set: bool,
    mode: Option<&CopyModeState>,
    linked_sessions: usize,
    group_info: (bool, usize, String),
    session_names: &str,
    window_names: &str,
    options: &HashMap<String, String>,
    format: &str,
) -> String {
    let mode_active = pane.panes_mode || mode.is_some();
    let mode_name = if pane.panes_mode {
        "panes-mode"
    } else {
        mode.map(CopyModeState::pane_mode).unwrap_or("")
    };
    let (cursor_y, cursor_x) = pane.parser.screen().cursor_position();
    let (copy_cursor_word, copy_cursor_line, copy_cursor_hyperlink) = mode.map_or_else(
        || (String::new(), String::new(), String::new()),
        |mode| {
            // `format_pane` is also used by immutable display/list requests,
            // while copy-mode's history helpers temporarily move a parser's
            // scrollback viewport. Rebuild a private parser from the retained
            // PTY stream so format expansion cannot mutate the live pane.
            let mut parser = copy_source_parser(pane, 10_000).unwrap_or_else(|| {
                let mut parser = Parser::new(pane.rect.rows.max(1), pane.rect.cols.max(1), 10_000);
                if pane.raw_output.is_empty() {
                    parser.process(pane.parser.screen().contents().as_bytes());
                } else {
                    terminal::replay(&mut parser, &pane.raw_output);
                }
                parser
            });
            let raw_output = pane
                .copy_source
                .as_ref()
                .map_or(pane.raw_output.as_slice(), |source| {
                    source.raw_output.as_slice()
                });
            let (viewport_row, viewport_col) = mode.cursor_viewport(&mut parser);
            (
                mode.cursor_word(&mut parser),
                mode.cursor_line(&mut parser),
                mouse_hyperlink_at(raw_output, viewport_row, viewport_col, pane.rect),
            )
        },
    );
    let (grouped, group_size, group_list) = group_info;
    let values = vec![
        ("pid", std::process::id().to_string()),
        ("session_id", format!("${}", session.id)),
        ("session_name", session.name.clone()),
        ("session_windows", session.windows.len().to_string()),
        ("session_path", session.cwd.clone().unwrap_or_default()),
        ("session_active", "1".to_owned()),
        (
            "session_grouped",
            if grouped { "1" } else { "0" }.to_owned(),
        ),
        ("session_group_size", group_size.to_string()),
        ("session_group_list", group_list),
        ("__session_names", session_names.to_owned()),
        ("window_index", window.index.to_string()),
        ("window_id", format!("@{}", window.id)),
        ("window_name", window.name.clone()),
        ("__window_names", window_names.to_owned()),
        ("window_panes", window.panes.len().to_string()),
        (
            "window_last_flag",
            if session.last_window == Some(window.index) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "window_linked",
            if linked_sessions > 1 { "1" } else { "0" }.to_owned(),
        ),
        ("window_linked_sessions", linked_sessions.to_string()),
        ("window_width", window.size.cols.to_string()),
        ("window_height", window.size.rows.to_string()),
        (
            "window_zoomed_flag",
            if window.zoomed { "1" } else { "0" }.to_owned(),
        ),
        (
            "window_bell_flag",
            if window.bell_alert { "1" } else { "0" }.to_owned(),
        ),
        ("window_flags", window_alert_flags(window)),
        (
            "window_active",
            if window.active_pane == pane.id {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "pane_active",
            if window.active_pane == pane.id {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        ("pane_id", format!("%{}", pane.id)),
        ("pane_marked", if marked { "1" } else { "0" }.to_owned()),
        (
            "pane_marked_set",
            if marked_set { "1" } else { "0" }.to_owned(),
        ),
        ("pane_index", pane.index.to_string()),
        (
            "pane_flags",
            if pane.dead {
                "Z"
            } else if window.active_pane == pane.id {
                "*"
            } else {
                ""
            }
            .to_owned(),
        ),
        (
            "pane_last",
            if window.last_pane == Some(pane.id) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "pane_at_top",
            if pane.rect.y == 0 { "1" } else { "0" }.to_owned(),
        ),
        (
            "pane_at_bottom",
            if pane.rect.y.saturating_add(pane.rect.rows) == window.size.rows {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "pane_at_left",
            if pane.rect.x == 0 { "1" } else { "0" }.to_owned(),
        ),
        (
            "pane_at_right",
            if pane.rect.x.saturating_add(pane.rect.cols) == window.size.cols {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        ("pane_width", pane.rect.cols.to_string()),
        ("pane_height", pane.rect.rows.to_string()),
        ("pane_left", pane.rect.x.to_string()),
        ("pane_top", pane.rect.y.to_string()),
        (
            "pane_right",
            pane.rect
                .x
                .saturating_add(pane.rect.cols.saturating_sub(1))
                .to_string(),
        ),
        (
            "pane_bottom",
            pane.rect
                .y
                .saturating_add(pane.rect.rows.saturating_sub(1))
                .to_string(),
        ),
        ("pane_current_command", pane.command.clone()),
        ("pane_start_command", pane.command.clone()),
        (
            "pane_start_command_list",
            pane.command_args
                .iter()
                .map(|argument| shell_quote_argument(argument))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        (
            "pane_current_path",
            pane.current_path
                .clone()
                .or_else(|| session.cwd.clone())
                .unwrap_or_default(),
        ),
        (
            "pane_start_path",
            pane.start_path
                .clone()
                .or_else(|| session.cwd.clone())
                .unwrap_or_default(),
        ),
        ("cursor_x", cursor_x.to_string()),
        ("cursor_y", cursor_y.to_string()),
        ("__pane_content", pane_content_for_format(pane)),
        (
            "pane_pid",
            if pane.pty.is_empty() {
                String::new()
            } else {
                pane.pty.pid().to_string()
            },
        ),
        ("pane_title", pane.title.clone()),
        (
            "pane_synchronized",
            if window.synchronize_panes { "1" } else { "0" }.to_owned(),
        ),
        (
            "pane_input_off",
            if pane.enabled { "0" } else { "1" }.to_owned(),
        ),
        ("pane_dead", if pane.dead { "1" } else { "0" }.to_owned()),
        (
            "pane_in_mode",
            if mode_active { "1" } else { "0" }.to_owned(),
        ),
        ("pane_mode", mode_name.to_owned()),
        (
            "selection_present",
            if mode.is_some_and(CopyModeState::selection_present) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "selection_active",
            if mode.is_some_and(CopyModeState::selection_is_active) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "selection_mode",
            mode.map_or_else(
                || "char".to_owned(),
                |mode| mode.selection_mode_name().to_owned(),
            ),
        ),
        (
            "scroll_position",
            mode.map_or_else(|| "0".to_owned(), |mode| mode.scroll_position().to_string()),
        ),
        (
            "copy_cursor_rectangle",
            if mode.is_some_and(CopyModeState::rectangle_selection) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "copy_cursor_x",
            mode.map_or_else(|| "0".to_owned(), |mode| mode.cursor_x.to_string()),
        ),
        (
            "copy_cursor_y",
            mode.map_or_else(|| "0".to_owned(), |mode| mode.cursor.row.to_string()),
        ),
        ("copy_cursor_word", copy_cursor_word),
        ("copy_cursor_line", copy_cursor_line),
        // Hyperlink identity comes from the retained PTY stream because vt100
        // 0.16 exposes only the visual cell contents.
        ("copy_cursor_hyperlink", copy_cursor_hyperlink),
        (
            "copy_line_numbers",
            if mode.is_some_and(CopyModeState::line_numbers) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "copy_position_hidden",
            if mode.is_some_and(|mode| mode.hide_position) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "refresh_active",
            if mode.is_some_and(CopyModeState::refresh_active) {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
    ];
    render_format_with_options(format, &values, options)
}

fn buffer_values(buffer: &Buffer) -> Vec<(&'static str, String)> {
    let sample = buffer
        .data
        .split(|byte| *byte == b'\n')
        .next()
        .map(String::from_utf8_lossy)
        .unwrap_or_default();
    vec![
        ("buffer_name", buffer.name.clone()),
        ("buffer_size", buffer.data.len().to_string()),
        ("buffer_sample", sample.into_owned()),
        ("buffer_created", buffer.created.to_string()),
        ("buffer_full", "0".to_owned()),
    ]
}

fn buffer_filter(buffer: &Buffer, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let value = render_format(filter, &buffer_values(buffer));
    !value.is_empty() && value != "0" && value != "false"
}

fn format_buffer(buffer: &Buffer, format: &str, options: &HashMap<String, String>) -> String {
    render_format_with_options(format, &buffer_values(buffer), options)
}

fn parse_tree_mode_command(
    arguments: &[String],
) -> Result<
    (
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        bool,
        bool,
        bool,
    ),
    String,
> {
    let mut target = None;
    let mut format = None;
    let mut filter = None;
    let mut sort = "index".to_owned();
    let mut reverse = false;
    let mut hide_source = false;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-tree -t requires a target".to_owned())?
                        .clone(),
                );
            }
            "-F" => {
                index += 1;
                format = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-tree -F requires a format".to_owned())?
                        .clone(),
                );
            }
            "-f" => {
                index += 1;
                filter = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-tree -f requires a filter".to_owned())?
                        .clone(),
                );
            }
            "-O" => {
                index += 1;
                sort = arguments
                    .get(index)
                    .ok_or_else(|| "choose-tree -O requires a sort order".to_owned())?
                    .clone();
            }
            "-r" => reverse = true,
            "-h" => hide_source = true,
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-tree option: {value}")),
        }
        index += 1;
    }
    Ok((
        target,
        format,
        filter,
        sort,
        reverse,
        hide_source,
        kill_on_exit,
    ))
}

fn parse_panes_mode_command(
    arguments: &[String],
) -> Result<
    (
        Option<String>,
        Option<String>,
        bool,
        bool,
        Vec<String>,
        bool,
    ),
    String,
> {
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
                target = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "display-panes -t requires a target".to_owned())?
                        .clone(),
                );
            }
            "-s" => {
                index += 1;
                source = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "display-panes -s requires a source".to_owned())?
                        .clone(),
                );
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
    Ok((target, source, no_zoom, no_mode, command, kill_on_exit))
}

fn parse_buffer_mode_command(
    arguments: &[String],
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
    let mut format = None;
    let mut filter = None;
    let mut sort = "index".to_owned();
    let mut reverse = false;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-buffer -t requires a target".to_owned())?
                        .clone(),
                );
            }
            "-F" => {
                index += 1;
                format = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-buffer -F requires a format".to_owned())?
                        .clone(),
                );
            }
            "-f" => {
                index += 1;
                filter = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-buffer -f requires a filter".to_owned())?
                        .clone(),
                );
            }
            "-O" => {
                index += 1;
                sort = arguments
                    .get(index)
                    .ok_or_else(|| "choose-buffer -O requires a sort order".to_owned())?
                    .clone();
            }
            "-r" => reverse = true,
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-buffer option: {value}")),
        }
        index += 1;
    }
    Ok((target, format, filter, sort, reverse, kill_on_exit))
}

fn parse_client_mode_command(
    arguments: &[String],
) -> Result<(Option<String>, Option<String>, Option<String>, bool), String> {
    let mut target = None;
    let mut format = None;
    let mut filter = None;
    let mut kill_on_exit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "-t" => {
                index += 1;
                target = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-client -t requires a target".to_owned())?
                        .clone(),
                );
            }
            "-F" => {
                index += 1;
                format = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-client -F requires a format".to_owned())?
                        .clone(),
                );
            }
            "-f" => {
                index += 1;
                filter = Some(
                    arguments
                        .get(index)
                        .ok_or_else(|| "choose-client -f requires a filter".to_owned())?
                        .clone(),
                );
            }
            "-k" => kill_on_exit = true,
            "-N" | "-Z" | "-G" => {}
            value => return Err(format!("unknown choose-client option: {value}")),
        }
        index += 1;
    }
    Ok((target, format, filter, kill_on_exit))
}

fn parse_copy_action(value: &str) -> Result<CopyAction, String> {
    let (name, argument) = value
        .split_once('\0')
        .map_or((value, None), |(name, argument)| (name, Some(argument)));
    let (name, flags) = name
        .split_once('\x1d')
        .map_or((name, ""), |(name, flags)| (name, flags));
    if flags.chars().any(|flag| !matches!(flag, 'P' | 'C')) {
        return Err(format!("unknown copy-mode flag set: {flags}"));
    }
    let set_paste = !flags.contains('P');
    let set_clipboard = !flags.contains('C');
    match name {
        "cancel" => Ok(CopyAction::Cancel),
        "history-top" => Ok(CopyAction::HistoryTop),
        "history-bottom" => Ok(CopyAction::HistoryBottom),
        "start-of-line" => Ok(CopyAction::StartOfLine),
        "end-of-line" => Ok(CopyAction::EndOfLine),
        "begin-selection" => Ok(CopyAction::BeginSelection),
        "select-line" => Ok(CopyAction::SelectLine),
        "select-word" => Ok(CopyAction::SelectWord),
        "clear-selection" => Ok(CopyAction::ClearSelection),
        "stop-selection" => Ok(CopyAction::StopSelection),
        "other-end" => Ok(CopyAction::OtherEnd),
        "cursor-up" => Ok(CopyAction::CursorUp),
        "cursor-down" => Ok(CopyAction::CursorDown),
        "cursor-down-and-cancel" => Ok(CopyAction::CursorDownAndCancel),
        "cursor-left" => Ok(CopyAction::CursorLeft),
        "cursor-right" => Ok(CopyAction::CursorRight),
        "scroll-up" => Ok(CopyAction::ScrollUp),
        "scroll-down" => Ok(CopyAction::ScrollDown),
        "scroll-top" => Ok(CopyAction::ScrollTop),
        "scroll-bottom" => Ok(CopyAction::ScrollBottom),
        "scroll-middle" => Ok(CopyAction::ScrollMiddle),
        "scroll-to-mouse" => Ok(CopyAction::ScrollToMouse(None)),
        "recentre-top-bottom" => Ok(CopyAction::RecenterTopBottom),
        "page-up" => Ok(CopyAction::PageUp),
        "page-down" => Ok(CopyAction::PageDown),
        "page-down-and-cancel" => Ok(CopyAction::PageDownAndCancel),
        "copy-selection" => Ok(parse_copy_selection_action(
            argument,
            true,
            false,
            set_paste,
            set_clipboard,
        )),
        "copy-selection-no-clear" => Ok(parse_copy_selection_action(
            argument,
            false,
            false,
            set_paste,
            set_clipboard,
        )),
        "copy-selection-and-cancel" => Ok(parse_copy_selection_action(
            argument,
            true,
            true,
            set_paste,
            set_clipboard,
        )),
        "copy-end-of-line" => Ok(parse_copy_line_action(
            argument,
            false,
            false,
            set_paste,
            set_clipboard,
        )),
        "copy-end-of-line-and-cancel" => Ok(parse_copy_line_action(
            argument,
            false,
            true,
            set_paste,
            set_clipboard,
        )),
        "copy-line" => Ok(parse_copy_line_action(
            argument,
            true,
            false,
            set_paste,
            set_clipboard,
        )),
        "copy-line-and-cancel" => Ok(parse_copy_line_action(
            argument,
            true,
            true,
            set_paste,
            set_clipboard,
        )),
        "copy-pipe-no-clear" => {
            parse_copy_pipe_action(argument, false, false, true, set_paste, set_clipboard)
        }
        "copy-pipe" => {
            parse_copy_pipe_action(argument, true, false, true, set_paste, set_clipboard)
        }
        "copy-pipe-and-cancel" => {
            parse_copy_pipe_action(argument, true, true, true, set_paste, set_clipboard)
        }
        "copy-pipe-end-of-line" => {
            parse_copy_pipe_line_action(argument, false, false, set_paste, set_clipboard)
        }
        "copy-pipe-end-of-line-and-cancel" => {
            parse_copy_pipe_line_action(argument, false, true, set_paste, set_clipboard)
        }
        "copy-pipe-line" => {
            parse_copy_pipe_line_action(argument, true, false, set_paste, set_clipboard)
        }
        "copy-pipe-line-and-cancel" => {
            parse_copy_pipe_line_action(argument, true, true, set_paste, set_clipboard)
        }
        "pipe-no-clear" => {
            parse_copy_pipe_action(argument, false, false, false, set_paste, set_clipboard)
        }
        "pipe" => parse_copy_pipe_action(argument, true, false, false, set_paste, set_clipboard),
        "pipe-and-cancel" => {
            parse_copy_pipe_action(argument, true, true, false, set_paste, set_clipboard)
        }
        "append-selection" => Ok(CopyAction::AppendSelection),
        "append-selection-and-cancel" => Ok(CopyAction::AppendSelectionAndCancel),
        "rectangle-toggle" => Ok(CopyAction::RectangleToggle),
        "rectangle-on" => Ok(CopyAction::RectangleOn),
        "rectangle-off" => Ok(CopyAction::RectangleOff),
        "selection-mode" => match argument {
            None | Some("char" | "c") => Ok(CopyAction::SelectionMode(SelectionMode::Char)),
            Some("word" | "w") => Ok(CopyAction::SelectionMode(SelectionMode::Word)),
            Some("line" | "l") => Ok(CopyAction::SelectionMode(SelectionMode::Line)),
            Some(value) => Err(format!("unknown selection mode: {value}")),
        },
        "top-line" => Ok(CopyAction::TopLine),
        "middle-line" => Ok(CopyAction::MiddleLine),
        "bottom-line" => Ok(CopyAction::BottomLine),
        "cursor-centre-vertical" => Ok(CopyAction::CursorCentreVertical),
        "halfpage-up" => Ok(CopyAction::HalfPageUp),
        "halfpage-down" => Ok(CopyAction::HalfPageDown),
        "halfpage-down-and-cancel" => Ok(CopyAction::HalfPageDownAndCancel),
        "back-to-indentation" => Ok(CopyAction::BackToIndentation),
        "cursor-centre-horizontal" => Ok(CopyAction::CursorCentreHorizontal),
        "scroll-down-and-cancel" => Ok(CopyAction::ScrollDownAndCancel),
        "scroll-exit-on" => Ok(CopyAction::ScrollExitOn),
        "scroll-exit-off" => Ok(CopyAction::ScrollExitOff),
        "scroll-exit-toggle" => Ok(CopyAction::ScrollExitToggle),
        "set-mark" => Ok(CopyAction::SetMark),
        "jump-to-mark" => Ok(CopyAction::JumpToMark),
        "jump-forward" => argument
            .map(|argument| CopyAction::JumpForward(argument.to_owned()))
            .ok_or_else(|| "jump-forward requires a character".to_owned()),
        "jump-backward" => argument
            .map(|argument| CopyAction::JumpBackward(argument.to_owned()))
            .ok_or_else(|| "jump-backward requires a character".to_owned()),
        "jump-to-forward" => argument
            .map(|argument| CopyAction::JumpToForward(argument.to_owned()))
            .ok_or_else(|| "jump-to-forward requires a character".to_owned()),
        "jump-to-backward" => argument
            .map(|argument| CopyAction::JumpToBackward(argument.to_owned()))
            .ok_or_else(|| "jump-to-backward requires a character".to_owned()),
        "jump-again" => Ok(CopyAction::JumpAgain),
        "jump-reverse" => Ok(CopyAction::JumpReverse),
        "previous-paragraph" => Ok(CopyAction::PreviousParagraph),
        "next-paragraph" => Ok(CopyAction::NextParagraph),
        "previous-matching-bracket" => Ok(CopyAction::PreviousMatchingBracket),
        "next-matching-bracket" => Ok(CopyAction::NextMatchingBracket),
        "search-forward" => argument
            .map(|argument| CopyAction::SearchForward(argument.to_owned()))
            .ok_or_else(|| "search-forward requires a search string".to_owned()),
        "search-forward-text" => argument
            .map(|argument| CopyAction::SearchForwardText(argument.to_owned()))
            .ok_or_else(|| "search-forward-text requires a search string".to_owned()),
        "search-forward-incremental" => {
            parse_incremental_search_action(argument, true, "search-forward-incremental")
        }
        "search-backward" => argument
            .map(|argument| CopyAction::SearchBackward(argument.to_owned()))
            .ok_or_else(|| "search-backward requires a search string".to_owned()),
        "search-backward-text" => argument
            .map(|argument| CopyAction::SearchBackwardText(argument.to_owned()))
            .ok_or_else(|| "search-backward-text requires a search string".to_owned()),
        "search-backward-incremental" => {
            parse_incremental_search_action(argument, false, "search-backward-incremental")
        }
        "next-prompt" => Ok(CopyAction::NextPrompt),
        "previous-prompt" => Ok(CopyAction::PreviousPrompt),
        "refresh-from-pane" | "refresh-now" => Ok(CopyAction::RefreshFromPane),
        "refresh-on" => Ok(CopyAction::RefreshOn),
        "refresh-off" => Ok(CopyAction::RefreshOff),
        "refresh-toggle" => Ok(CopyAction::RefreshToggle),
        "line-numbers-on" => Ok(CopyAction::LineNumbersOn),
        "line-numbers-off" => Ok(CopyAction::LineNumbersOff),
        "line-numbers-toggle" => Ok(CopyAction::LineNumbersToggle),
        "toggle-position" => Ok(CopyAction::TogglePosition),
        "search-again" => Ok(CopyAction::SearchAgain),
        "search-reverse" => Ok(CopyAction::SearchReverse),
        "goto-line" => argument
            .ok_or_else(|| "goto-line requires a line number".to_owned())?
            .parse::<usize>()
            .map(CopyAction::GotoLine)
            .map_err(|_| "goto-line requires a positive integer".to_owned()),
        "next-word" => Ok(CopyAction::NextWord),
        "next-word-end" => Ok(CopyAction::NextWordEnd),
        "previous-word" => Ok(CopyAction::PreviousWord),
        "previous-space" => Ok(CopyAction::PreviousSpace),
        "next-space" => Ok(CopyAction::NextSpace),
        "next-space-end" => Ok(CopyAction::NextSpaceEnd),
        _ => Err(format!("unknown copy-mode command: {value}")),
    }
}

fn parse_copy_selection_action(
    argument: Option<&str>,
    clear: bool,
    cancel: bool,
    set_paste: bool,
    set_clipboard: bool,
) -> CopyAction {
    if argument.is_some() || !set_paste || !set_clipboard {
        CopyAction::CopySelectionWithOptions {
            prefix: argument.map(str::to_owned),
            clear,
            cancel,
            set_paste,
            set_clipboard,
        }
    } else {
        match (clear, cancel) {
            (true, true) => CopyAction::CopySelectionAndCancel,
            (true, false) => CopyAction::CopySelection,
            (false, false) => CopyAction::CopySelectionNoClear,
            (false, true) => unreachable!("copy-selection-no-clear cannot cancel"),
        }
    }
}

fn parse_copy_line_action(
    argument: Option<&str>,
    whole_line: bool,
    cancel: bool,
    set_paste: bool,
    set_clipboard: bool,
) -> CopyAction {
    if argument.is_some() || !set_paste || !set_clipboard {
        CopyAction::CopyLineWithOptions {
            prefix: argument.map(str::to_owned),
            whole_line,
            cancel,
            set_paste,
            set_clipboard,
        }
    } else if whole_line && cancel {
        CopyAction::CopyLineAndCancel
    } else if whole_line {
        CopyAction::CopyLine
    } else if cancel {
        CopyAction::CopyEndOfLineAndCancel
    } else {
        CopyAction::CopyEndOfLine
    }
}

fn parse_copy_pipe_arguments(argument: Option<&str>) -> Result<(String, Option<String>), String> {
    // `send-keys -X` has one string argument, so the command and optional
    // automatic-buffer prefix are separated before this request reaches the
    // server. A command may still contain ordinary spaces unchanged.
    let Some(argument) = argument else {
        return Ok((String::new(), None));
    };
    let mut values = argument.split('\x1e');
    let command = values.next().unwrap_or_default().to_owned();
    let prefix = values.next().map(str::to_owned);
    if values.next().is_some() {
        return Err("copy-pipe accepts at most a command and buffer prefix".to_owned());
    }
    Ok((command, prefix))
}

fn parse_copy_pipe_action(
    argument: Option<&str>,
    clear: bool,
    cancel: bool,
    store: bool,
    set_paste: bool,
    set_clipboard: bool,
) -> Result<CopyAction, String> {
    let (command, prefix) = parse_copy_pipe_arguments(argument)?;
    if prefix.is_some() || !set_paste || !set_clipboard {
        Ok(CopyAction::CopyPipeWithOptions {
            command,
            prefix,
            clear,
            cancel,
            store,
            set_paste,
            set_clipboard,
        })
    } else {
        Ok(CopyAction::CopyPipe {
            command,
            clear,
            cancel,
            store,
        })
    }
}

fn parse_copy_pipe_line_action(
    argument: Option<&str>,
    whole_line: bool,
    cancel: bool,
    set_paste: bool,
    set_clipboard: bool,
) -> Result<CopyAction, String> {
    let (command, prefix) = parse_copy_pipe_arguments(argument)?;
    if prefix.is_some() || !set_paste || !set_clipboard {
        Ok(CopyAction::CopyPipeLineWithOptions {
            command,
            prefix,
            whole_line,
            cancel,
            set_paste,
            set_clipboard,
        })
    } else if whole_line {
        Ok(CopyAction::CopyPipeLine { command, cancel })
    } else {
        Ok(CopyAction::CopyPipeEndOfLine { command, cancel })
    }
}

fn parse_incremental_search_action(
    argument: Option<&str>,
    forward: bool,
    command: &str,
) -> Result<CopyAction, String> {
    let argument = argument.ok_or_else(|| format!("{command} requires a search string"))?;
    let (forward, search) = match argument.strip_prefix(['+', '-', '=']) {
        Some(search) if argument.starts_with('+') => (true, search),
        Some(search) if argument.starts_with('-') => (false, search),
        Some(search) => (forward, search),
        None => (forward, argument),
    };
    Ok(if forward {
        CopyAction::SearchForwardIncremental(search.to_owned())
    } else {
        CopyAction::SearchBackwardIncremental(search.to_owned())
    })
}

fn run_copy_pipe(
    command: &str,
    data: &[u8],
    environment: &HashMap<String, String>,
) -> CommandResult {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .envs(environment.iter())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("copy pipe: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .map_err(|error| format!("copy pipe: {error}"))?;
    }
    let _ = child.wait();
    Ok(String::new())
}

fn copy_input_actions(
    state: &mut ServerState,
    pane_id: u64,
    bytes: &[u8],
) -> (Vec<(CopyAction, usize)>, usize) {
    let keys = state
        .find_pane(pane_id)
        .and_then(|pane| pane.copy_mode.as_ref())
        .map(|mode| mode.keys)
        .unwrap_or(CopyModeKeys::Emacs);
    let mut actions = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some((action, consumed)) = state
            .find_pane_mut(pane_id)
            .and_then(|pane| pane.copy_mode.as_mut())
            .and_then(|mode| mode.feed_prompt(&bytes[index..]))
        {
            index += consumed;
            if let Some(action) = action {
                let repeat = state
                    .find_pane_mut(pane_id)
                    .and_then(|pane| pane.copy_mode.as_mut())
                    .map_or(1, |mode| mode.take_repeat());
                actions.push((action, repeat));
            }
            continue;
        }
        let is_repeat = state
            .find_pane_mut(pane_id)
            .and_then(|pane| pane.copy_mode.as_mut())
            .is_some_and(|mode| mode.feed_repeat_digit(bytes[index]));
        if is_repeat {
            index += 1;
            continue;
        }
        let action = if bytes[index..].starts_with(b"\x1b[A") {
            index += 3;
            Some(CopyAction::CursorUp)
        } else if bytes[index..].starts_with(b"\x1b[B") {
            index += 3;
            Some(CopyAction::CursorDown)
        } else if bytes[index..].starts_with(b"\x1b[C") {
            index += 3;
            Some(CopyAction::CursorRight)
        } else if bytes[index..].starts_with(b"\x1b[D") {
            index += 3;
            Some(CopyAction::CursorLeft)
        } else if bytes[index..].starts_with(b"\x1b[1~")
            || bytes[index..].starts_with(b"\x1b[H")
            || bytes[index..].starts_with(b"\x1bOH")
        {
            index += if bytes[index..].starts_with(b"\x1b[1~") {
                4
            } else {
                3
            };
            Some(CopyAction::StartOfLine)
        } else if bytes[index..].starts_with(b"\x1b[4~")
            || bytes[index..].starts_with(b"\x1b[F")
            || bytes[index..].starts_with(b"\x1bOF")
        {
            index += if bytes[index..].starts_with(b"\x1b[4~") {
                4
            } else {
                3
            };
            Some(CopyAction::EndOfLine)
        } else if bytes[index..].starts_with(b"\x1b[5~") {
            index += 4;
            Some(CopyAction::PageUp)
        } else if bytes[index..].starts_with(b"\x1b[6~") {
            index += 4;
            Some(CopyAction::PageDown)
        } else if matches!(keys, CopyModeKeys::Emacs) && bytes[index..].starts_with(b"\x1b[1;3A") {
            index += 6;
            Some(CopyAction::HalfPageUp)
        } else if matches!(keys, CopyModeKeys::Emacs) && bytes[index..].starts_with(b"\x1b[1;3B") {
            index += 6;
            Some(CopyAction::HalfPageDown)
        } else if bytes[index..].starts_with(b"\x1b[1;5A") {
            index += 6;
            Some(CopyAction::ScrollUp)
        } else if bytes[index..].starts_with(b"\x1b[1;5B") {
            index += 6;
            Some(CopyAction::ScrollDown)
        } else if matches!(keys, CopyModeKeys::Emacs) && bytes[index..].starts_with(b"\x1b[1;7A") {
            index += 6;
            Some(CopyAction::PreviousPrompt)
        } else if matches!(keys, CopyModeKeys::Emacs) && bytes[index..].starts_with(b"\x1b[1;7B") {
            index += 6;
            Some(CopyAction::NextPrompt)
        } else if matches!(keys, CopyModeKeys::Emacs)
            && bytes[index] == 0x1b
            && index + 1 < bytes.len()
            && (b'1'..=b'9').contains(&bytes[index + 1])
        {
            let digit = bytes[index + 1];
            index += 2;
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.feed_repeat_digit(digit);
            }
            None
        } else if bytes[index] == 0x1b && index + 1 < bytes.len() {
            index += 2;
            Some(match (keys, bytes[index - 1]) {
                (CopyModeKeys::Vi, b'b') | (CopyModeKeys::Emacs, b'b') => CopyAction::PreviousWord,
                (CopyModeKeys::Vi, b'f') | (CopyModeKeys::Emacs, b'f') => CopyAction::NextWordEnd,
                (CopyModeKeys::Emacs, 0x02) => CopyAction::PreviousMatchingBracket,
                (CopyModeKeys::Emacs, 0x06) => CopyAction::NextMatchingBracket,
                (_, b'v') => CopyAction::PageUp,
                (_, b'<') => CopyAction::HistoryTop,
                (_, b'>') => CopyAction::HistoryBottom,
                (CopyModeKeys::Emacs, b'{') => CopyAction::PreviousParagraph,
                (CopyModeKeys::Emacs, b'}') => CopyAction::NextParagraph,
                (_, b'{') => CopyAction::PreviousWord,
                (_, b'}') => CopyAction::NextWord,
                (CopyModeKeys::Emacs, b'l') => CopyAction::CursorCentreHorizontal,
                (_, b'x') => CopyAction::JumpToMark,
                (CopyModeKeys::Emacs, b'R') => CopyAction::TopLine,
                (CopyModeKeys::Emacs, b'r') => CopyAction::MiddleLine,
                (CopyModeKeys::Emacs, b'm') => CopyAction::BackToIndentation,
                (CopyModeKeys::Emacs, b'w') => CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                _ => continue,
            })
        } else if matches!(keys, CopyModeKeys::Vi) && matches!(bytes[index], b'#' | b'*') {
            let forward = bytes[index] == b'*';
            index += 1;
            let history_limit = state.history_limit;
            let word = state
                .find_pane_mut(pane_id)
                .map_or_else(String::new, |pane| {
                    let mut source_parser = copy_source_parser(pane, history_limit);
                    let parser = source_parser
                        .as_mut()
                        .map_or(&mut pane.parser, |parser| parser);
                    pane.copy_mode
                        .as_ref()
                        .map_or_else(String::new, |mode| mode.cursor_word(parser))
                });
            Some(if forward {
                CopyAction::SearchForward(word)
            } else {
                CopyAction::SearchBackward(word)
            })
        } else if matches!(keys, CopyModeKeys::Vi) && bytes[index] == b':' {
            index += 1;
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.begin_prompt(CopyPromptKind::GotoLine);
            }
            None
        } else if matches!(keys, CopyModeKeys::Emacs) && bytes[index] == b'g' {
            index += 1;
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.begin_prompt(CopyPromptKind::GotoLine);
            }
            None
        } else if matches!(bytes[index], b'f' | b'F' | b't' | b'T') {
            let key = bytes[index];
            index += 1;
            let kind = match key {
                b'f' => CopyPromptKind::JumpForward,
                b'F' => CopyPromptKind::JumpBackward,
                b't' => CopyPromptKind::JumpToForward,
                b'T' => CopyPromptKind::JumpToBackward,
                _ => unreachable!(),
            };
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.begin_prompt(kind);
            }
            None
        } else if matches!(keys, CopyModeKeys::Vi) && matches!(bytes[index], b'/' | b'?') {
            let forward = bytes[index] == b'/';
            index += 1;
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.begin_prompt(if forward {
                    CopyPromptKind::SearchForward
                } else {
                    CopyPromptKind::SearchBackward
                });
            }
            None
        } else if matches!(keys, CopyModeKeys::Emacs) && matches!(bytes[index], 0x12 | 0x13) {
            let forward = bytes[index] == 0x13;
            index += 1;
            if let Some(mode) = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
            {
                mode.begin_prompt(if forward {
                    CopyPromptKind::SearchForwardIncremental
                } else {
                    CopyPromptKind::SearchBackwardIncremental
                });
            }
            None
        } else {
            let byte = bytes[index];
            index += 1;
            Some(match (keys, byte) {
                (_, 3) | (CopyModeKeys::Vi, b'q') => CopyAction::Cancel,
                (CopyModeKeys::Vi, b'k') | (CopyModeKeys::Emacs, 0x10) => CopyAction::CursorUp,
                (CopyModeKeys::Vi, b'j') | (CopyModeKeys::Emacs, 0x0e) => CopyAction::CursorDown,
                (CopyModeKeys::Vi, b'h') | (CopyModeKeys::Emacs, 0x02) => CopyAction::CursorLeft,
                (CopyModeKeys::Vi, b'l') | (CopyModeKeys::Emacs, 0x06) => CopyAction::CursorRight,
                (CopyModeKeys::Vi, b'g') => CopyAction::HistoryTop,
                (CopyModeKeys::Vi, b'G') => CopyAction::HistoryBottom,
                (CopyModeKeys::Vi, b'v') => CopyAction::RectangleToggle,
                (CopyModeKeys::Emacs, 0) => CopyAction::BeginSelection,
                (CopyModeKeys::Vi, b'V') => CopyAction::SelectLine,
                (CopyModeKeys::Vi, b'y') => CopyAction::CopySelection,
                (CopyModeKeys::Vi, b'A') => CopyAction::AppendSelectionAndCancel,
                (CopyModeKeys::Vi, b'o') => CopyAction::OtherEnd,
                (CopyModeKeys::Vi, b',') | (CopyModeKeys::Emacs, b',') => CopyAction::JumpReverse,
                (CopyModeKeys::Vi, b';') | (CopyModeKeys::Emacs, b';') => CopyAction::JumpAgain,
                (CopyModeKeys::Vi, b'J') => CopyAction::ScrollDown,
                (CopyModeKeys::Vi, b'K') => CopyAction::ScrollUp,
                (CopyModeKeys::Vi, b'z') => CopyAction::ScrollMiddle,
                (CopyModeKeys::Vi, b'n') => CopyAction::SearchAgain,
                (CopyModeKeys::Vi, b'N') => CopyAction::SearchReverse,
                (CopyModeKeys::Vi, b'r') => CopyAction::RefreshFromPane,
                (CopyModeKeys::Vi, b'P') => CopyAction::TogglePosition,
                (CopyModeKeys::Vi, b'X') => CopyAction::SetMark,
                (CopyModeKeys::Vi, b'x') => CopyAction::JumpToMark,
                (CopyModeKeys::Vi, b'H') => CopyAction::TopLine,
                (CopyModeKeys::Vi, b'M') => CopyAction::MiddleLine,
                (CopyModeKeys::Vi, b'L') => CopyAction::BottomLine,
                (CopyModeKeys::Vi, b'^') => CopyAction::BackToIndentation,
                (CopyModeKeys::Vi, 0x04) => CopyAction::HalfPageDown,
                (CopyModeKeys::Vi, 0x15) => CopyAction::HalfPageUp,
                (CopyModeKeys::Vi, 0x02) => CopyAction::PageUp,
                (CopyModeKeys::Vi, 0x06) => CopyAction::PageDown,
                (CopyModeKeys::Vi, 0x05) => CopyAction::ScrollDown,
                (CopyModeKeys::Vi, 0x19) => CopyAction::ScrollUp,
                (CopyModeKeys::Vi, 0x08) => CopyAction::CursorLeft,
                (CopyModeKeys::Vi, 0x16) => CopyAction::RectangleToggle,
                (CopyModeKeys::Vi, 0x0a) => CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                (CopyModeKeys::Vi, 0x1b) => CopyAction::ClearSelection,
                (CopyModeKeys::Vi, b'B') => CopyAction::PreviousSpace,
                (CopyModeKeys::Vi, b'D') => CopyAction::CopyPipeEndOfLine {
                    command: String::new(),
                    cancel: true,
                },
                (CopyModeKeys::Vi, b'E') => CopyAction::NextSpaceEnd,
                (CopyModeKeys::Vi, b'W') => CopyAction::NextSpace,
                (CopyModeKeys::Vi, b'%') => CopyAction::NextMatchingBracket,
                (CopyModeKeys::Vi, b'{') => CopyAction::PreviousParagraph,
                (CopyModeKeys::Vi, b'}') => CopyAction::NextParagraph,
                (CopyModeKeys::Vi, 127) => CopyAction::CursorLeft,
                (CopyModeKeys::Emacs, 0x1b) => CopyAction::Cancel,
                (CopyModeKeys::Emacs, 0x01) => CopyAction::StartOfLine,
                (CopyModeKeys::Emacs, 0x05) => CopyAction::EndOfLine,
                (CopyModeKeys::Emacs, 0x07) => CopyAction::ClearSelection,
                (CopyModeKeys::Emacs, 0x0b) => CopyAction::CopyPipeEndOfLine {
                    command: String::new(),
                    cancel: true,
                },
                (CopyModeKeys::Emacs, 0x16) => CopyAction::PageDown,
                (CopyModeKeys::Emacs, 0x0c) => CopyAction::RecenterTopBottom,
                (CopyModeKeys::Emacs, 0x17) => CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                (CopyModeKeys::Emacs, b'L') => CopyAction::LineNumbersToggle,
                (CopyModeKeys::Emacs, b'P') => CopyAction::TogglePosition,
                (CopyModeKeys::Emacs, b'R') => CopyAction::RectangleToggle,
                (CopyModeKeys::Emacs, b'X') => CopyAction::SetMark,
                (CopyModeKeys::Emacs, b'n') => CopyAction::SearchAgain,
                (CopyModeKeys::Emacs, b'N') => CopyAction::SearchReverse,
                (CopyModeKeys::Emacs, b'r') => CopyAction::RefreshFromPane,
                (CopyModeKeys::Emacs, b' ') => CopyAction::PageDown,
                (CopyModeKeys::Vi, b' ') => CopyAction::BeginSelection,
                (CopyModeKeys::Vi, b'\r') => CopyAction::CopyPipe {
                    command: String::new(),
                    clear: true,
                    cancel: true,
                    store: true,
                },
                (_, b'\r') => CopyAction::CopySelectionAndCancel,
                (_, b'b') => CopyAction::PreviousWord,
                (_, b'w') => CopyAction::NextWord,
                (_, b'e') => CopyAction::NextWordEnd,
                (_, b'0') => CopyAction::StartOfLine,
                (_, b'$') => CopyAction::EndOfLine,
                _ => continue,
            })
        };
        if let Some(action) = action {
            let repeat = state
                .find_pane_mut(pane_id)
                .and_then(|pane| pane.copy_mode.as_mut())
                .map_or(1, |mode| mode.take_repeat());
            actions.push((action, repeat));
        }
    }
    (actions, index)
}

impl ServerState {
    fn find_pane_mut(&mut self, pane_id: u64) -> Option<&mut Pane> {
        self.sessions
            .iter_mut()
            .flat_map(|session| &mut session.windows)
            .flat_map(|window| &mut window.panes)
            .find(|pane| pane.id == pane_id)
    }
}

fn directional_pane(
    window: &Window,
    current_id: u64,
    current: Rect,
    direction: PaneDirection,
) -> Option<u64> {
    let center_x = u32::from(current.x) + u32::from(current.cols) / 2;
    let center_y = u32::from(current.y) + u32::from(current.rows) / 2;
    window
        .panes
        .iter()
        .filter(|pane| pane.id != current_id)
        .filter(|pane| match direction {
            PaneDirection::Left => pane.rect.x + pane.rect.cols <= current.x,
            PaneDirection::Right => pane.rect.x >= current.x + current.cols,
            PaneDirection::Up => pane.rect.y + pane.rect.rows <= current.y,
            PaneDirection::Down => pane.rect.y >= current.y + current.rows,
            _ => false,
        })
        .filter(|pane| match direction {
            PaneDirection::Left | PaneDirection::Right => {
                center_y >= u32::from(pane.rect.y)
                    && center_y < u32::from(pane.rect.y + pane.rect.rows)
            }
            PaneDirection::Up | PaneDirection::Down => {
                center_x >= u32::from(pane.rect.x)
                    && center_x < u32::from(pane.rect.x + pane.rect.cols)
            }
            _ => false,
        })
        .min_by_key(|pane| match direction {
            PaneDirection::Left => current.x.saturating_sub(pane.rect.x + pane.rect.cols),
            PaneDirection::Right => pane.rect.x.saturating_sub(current.x + current.cols),
            PaneDirection::Up => current.y.saturating_sub(pane.rect.y + pane.rect.rows),
            PaneDirection::Down => pane.rect.y.saturating_sub(current.y + current.rows),
            _ => 0,
        })
        .map(|pane| pane.id)
}

fn next_pane_id(window: &Window, current_id: u64, delta: isize) -> Option<u64> {
    if window.panes.is_empty() {
        return None;
    }
    let position = window.panes.iter().position(|pane| pane.id == current_id)? as isize;
    let next = (position + delta).rem_euclid(window.panes.len() as isize) as usize;
    Some(window.panes[next].id)
}

fn pane_iter(window: &Window) -> Vec<&Pane> {
    window.panes.iter().collect()
}

trait PaneSize {
    fn rect_size(&self) -> Size;
}

impl PaneSize for Pane {
    fn rect_size(&self) -> Size {
        Size::new(self.rect.cols.max(1), self.rect.rows.max(1))
    }
}

fn poisoned() -> io::Error {
    io::Error::other("server state lock is poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_path_decodes_osc7_uri_forms_headlessly() {
        assert_eq!(
            terminal_path(b"\x1b]7;file://localhost/tmp/a%20b\x07"),
            Some("/tmp/a b".to_owned())
        );
        assert_eq!(
            terminal_path(b"\x1b]7;file:///Users/example\x1b\\"),
            Some("/Users/example".to_owned())
        );
    }

    #[test]
    fn choose_tree_filters_renders_and_switches_clients_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("zzz"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create zzz session");
        state
            .split_window(
                &shared,
                Some("zzz:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split zzz session");
        state
            .create_session(
                &shared,
                Some("aaa"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create aaa session");
        let (_, client_id) = state
            .register_client(Some("aaa"), Size::new(40, 8))
            .expect("register tree client");

        state
            .enter_tree_mode(
                client_id,
                Some("aaa:0"),
                Some("#{session_name}:F"),
                Some("#{==:#{session_name},aaa}"),
                "index",
                false,
                false,
                false,
            )
            .expect("enter tree mode");
        let session_id = state.clients[&client_id].session_id;
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render tree mode");
        assert_eq!(
            rendered
                .windows(b"aaa:F".len())
                .filter(|window| *window == b"aaa:F")
                .count(),
            3
        );
        assert!(
            !rendered
                .windows(b"zzz:F".len())
                .any(|window| window == b"zzz:F")
        );

        state.handle_client_input(client_id, b"q", &shared);
        assert!(state.clients[&client_id].tree_mode.is_none());

        state
            .enter_tree_mode(
                client_id,
                Some("aaa:0"),
                Some("#{session_name}:F"),
                None,
                "index",
                false,
                false,
                false,
            )
            .expect("re-enter tree mode");
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render unfiltered tree mode");
        assert_eq!(
            rendered
                .windows(b"zzz:F".len())
                .filter(|window| *window == b"zzz:F")
                .count(),
            4
        );
        state.handle_client_input(client_id, b"gh", &shared);
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render collapsed tree mode");
        assert_eq!(
            rendered
                .windows(b"zzz:F".len())
                .filter(|window| *window == b"zzz:F")
                .count(),
            1
        );
        state.handle_client_input(client_id, b"l", &shared);
        state.handle_client_input(client_id, b"f#{==:#{session_name},zzz}\r", &shared);
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render prompted tree filter");
        assert_eq!(
            rendered
                .windows(b"zzz:F".len())
                .filter(|window| *window == b"zzz:F")
                .count(),
            4
        );
        state.handle_client_input(client_id, b"c", &shared);
        state
            .enter_tree_mode(
                client_id,
                Some("aaa:0"),
                Some("#{session_name}:F"),
                Some("#{==:#{session_name},nosuch}"),
                "index",
                false,
                true,
                false,
            )
            .expect("enter no-match tree mode");
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render no-match tree mode");
        assert!(
            rendered
                .windows(b"no matches".len())
                .any(|window| window == b"no matches")
        );
        state.handle_client_input(client_id, b"g\r", &shared);
        assert_eq!(state.clients[&client_id].session_id, state.sessions[0].id);
        assert!(state.clients[&client_id].tree_mode.is_none());

        state
            .new_window(
                &shared,
                Some("zzz"),
                None,
                true,
                None,
                false,
                None,
                false,
                false,
                false,
                true,
                &[],
                None,
            )
            .expect("add tree window for kill test");
        state
            .enter_tree_mode(
                client_id,
                Some("zzz:0"),
                Some("#{session_name}:G"),
                None,
                "index",
                false,
                false,
                false,
            )
            .expect("re-enter tree mode for kill test");
        state.handle_client_input(client_id, b"gjjjjx", &shared);
        let session_id = state.clients[&client_id].session_id;
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render tree confirmation");
        assert!(
            rendered
                .windows(b"Kill window 1".len())
                .any(|window| window == b"Kill window 1")
        );
        state.handle_client_input(client_id, b"y", &shared);
        assert_eq!(state.sessions[0].windows.len(), 1);
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render tree after kill");
        assert_eq!(
            rendered
                .windows(b"zzz:G".len())
                .filter(|window| *window == b"zzz:G")
                .count(),
            4
        );
        state.handle_client_input(client_id, b"q", &shared);
    }

    #[test]
    fn choose_buffer_and_client_modes_match_headless_selection_contract() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("aaa"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create buffer session");
        state
            .create_session(
                &shared,
                Some("bbb"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create client session");
        let (_, client_a) = state
            .register_client(Some("aaa"), Size::new(40, 8))
            .expect("register aaa client");
        let (_, client_b) = state
            .register_client(Some("bbb"), Size::new(40, 8))
            .expect("register bbb client");
        state.store_buffer(Some("bufa".to_owned()), b"hello log".to_vec(), false);
        state.store_buffer(Some("bufz".to_owned()), b"other".to_vec(), false);

        state
            .enter_buffer_mode(
                client_a,
                Some("aaa:0"),
                Some("B"),
                Some("#{m:*log*,#{buffer_sample}}"),
                "name",
                false,
                false,
            )
            .expect("enter buffer mode");
        let session_id = state.clients[&client_a].session_id;
        let rendered = state
            .render_session(session_id, Some(client_a))
            .expect("render buffer mode");
        assert!(
            rendered
                .windows(b"bufa: B".len())
                .any(|window| window == b"bufa: B")
        );
        assert!(
            !rendered
                .windows(b"bufz: B".len())
                .any(|window| window == b"bufz: B")
        );
        state.handle_client_input(client_a, b"q", &shared);

        state
            .enter_buffer_mode(
                client_a,
                Some("aaa:0"),
                Some("B"),
                Some("#{==:#{buffer_name},nosuch}"),
                "index",
                false,
                false,
            )
            .expect("enter no-match buffer mode");
        let rendered = state
            .render_session(session_id, Some(client_a))
            .expect("render no-match buffer mode");
        assert!(
            rendered
                .windows(b"no matches".len())
                .any(|window| window == b"no matches")
        );
        state.handle_client_input(client_a, b"\x14D", &shared);
        assert!(state.buffers.is_empty());
        state.handle_client_input(client_a, b"q", &shared);

        state.store_buffer(Some("bufa".to_owned()), b"paste".to_vec(), false);
        state
            .enter_buffer_mode(client_a, Some("aaa:0"), None, None, "index", false, false)
            .expect("re-enter buffer mode");
        state.handle_client_input(client_a, b"\r", &shared);
        assert!(state.clients[&client_a].buffer_mode.is_none());

        state
            .enter_client_mode(
                client_a,
                Some("aaa:0"),
                Some("C=#{client_session}"),
                Some("#{==:#{client_session},bbb}"),
                false,
            )
            .expect("enter client mode");
        let rendered = state
            .render_session(session_id, Some(client_a))
            .expect("render client mode");
        assert!(
            rendered
                .windows(b"client1: C=bbb".len())
                .any(|window| window == b"client1: C=bbb")
        );
        state.handle_client_input(client_a, b"\r", &shared);
        assert!(!state.clients.contains_key(&client_b));
        assert!(state.clients[&client_a].client_mode.is_none());
    }

    #[test]
    fn display_panes_mode_renders_labels_restores_zoom_and_selects_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("panes"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create panes session");
        state
            .split_window(
                &shared,
                Some("panes:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split panes session");
        state
            .set_global_option(
                "display-panes-format",
                "P#{pane_index}:#{pane_unzoomed_width}x#{pane_unzoomed_height}",
                false,
            )
            .expect("set display panes format");
        let (_, client_id) = state
            .register_client(Some("panes"), Size::new(40, 8))
            .expect("register panes client");
        let pane0 = state.sessions[0].windows[0].panes[0].id;
        let pane1 = state.sessions[0].windows[0].panes[1].id;
        let target = format!("%{pane0}");
        state
            .enter_panes_mode(client_id, Some(&target), None, false, None, false)
            .expect("enter panes mode");
        assert!(state.sessions[0].windows[0].zoomed);
        assert!(state.find_pane(pane0).is_some_and(|pane| pane.panes_mode));
        let session_id = state.clients[&client_id].session_id;
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render panes mode");
        assert!(
            rendered
                .windows(b"P0:".len())
                .any(|window| window == b"P0:")
        );
        assert!(
            rendered
                .windows(b"P1:".len())
                .any(|window| window == b"P1:")
        );
        assert_eq!(
            state
                .display_message(Some("panes:0.0"), "#{pane_mode} #{pane_in_mode}")
                .expect("format panes mode"),
            "panes-mode 1"
        );
        state.handle_client_input(client_id, b"q", &shared);
        assert!(!state.sessions[0].windows[0].zoomed);
        assert!(state.find_pane(pane0).is_some_and(|pane| !pane.panes_mode));

        state
            .enter_panes_mode(client_id, Some(&target), None, true, None, false)
            .expect("enter unzoomed panes mode");
        assert!(!state.sessions[0].windows[0].zoomed);
        state.handle_client_input(client_id, b"1", &shared);
        assert_eq!(state.sessions[0].windows[0].active_pane, pane1);
        assert!(state.clients[&client_id].panes_mode.is_none());

        state
            .set_global_option("@picked", "none", false)
            .expect("seed pane selection option");
        state
            .enter_panes_mode(
                client_id,
                Some(&target),
                None,
                true,
                Some(vec![
                    "set-option".to_owned(),
                    "-g".to_owned(),
                    "@picked".to_owned(),
                    "%%".to_owned(),
                ]),
                false,
            )
            .expect("enter panes selection command");
        state.handle_client_input(client_id, b"1", &shared);
        assert_eq!(
            state.global_options.get("@picked"),
            Some(&format!("%{pane1}"))
        );

        state
            .enter_copy_mode(Some(&target), None, false, false, false, false)
            .expect("enter copy mode before panes stack");
        state
            .enter_panes_mode(client_id, Some(&target), None, true, None, false)
            .expect("stack panes mode over copy mode");
        assert_eq!(
            state
                .display_message(Some("panes:0.0"), "#{pane_mode}")
                .expect("format stacked panes mode"),
            "panes-mode"
        );
        state.handle_client_input(client_id, b"q", &shared);
        assert!(
            state
                .find_pane(pane0)
                .is_some_and(|pane| { !pane.panes_mode && pane.copy_mode.is_some() })
        );
        state
            .execute_copy_action(pane0, CopyAction::Cancel, 1)
            .expect("cancel stacked copy mode");
        state
            .enter_panes_mode(client_id, Some(&target), None, true, None, false)
            .expect("enter panes mode before copy replacement");
        state
            .enter_copy_mode(Some(&target), None, false, false, false, false)
            .expect("replace panes mode with copy mode");
        assert!(
            state
                .find_pane(pane0)
                .is_some_and(|pane| { !pane.panes_mode && pane.copy_mode.is_some() })
        );
        state
            .execute_copy_action(pane0, CopyAction::Cancel, 1)
            .expect("cancel copy replacement");
    }

    #[test]
    fn rendered_split_panes_preserve_cell_styles_and_draw_separators_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("render-split"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create render split session");
        state
            .set_global_option("status", "off", false)
            .expect("hide render test status");
        state
            .split_window(
                &shared,
                Some("render-split:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split render session");
        let left = state.sessions[0].windows[0].panes[0].id;
        let right = state.sessions[0].windows[0].panes[1].id;
        state
            .find_pane_mut(left)
            .expect("left render pane")
            .parser
            .process(b"\x1b[1;42;31mR");
        state
            .find_pane_mut(right)
            .expect("right render pane")
            .parser
            .process(b"B");
        let session_id = state.sessions[0].id;
        let rendered = state
            .render_session(session_id, None)
            .expect("render split session");
        let mut terminal = Parser::new(6, 20, 100);
        terminal.process(&rendered);
        let screen = terminal.screen();
        let colored = screen.cell(0, 0).expect("colored cell");
        assert_eq!(colored.contents(), "R");
        assert_eq!(colored.fgcolor(), Color::Idx(1));
        assert_eq!(colored.bgcolor(), Color::Idx(2));
        assert!(colored.bold());
        assert_eq!(
            screen.cell(0, 10).expect("vertical separator").contents(),
            "│"
        );
        assert_eq!(
            screen.cell(0, 10).expect("vertical separator").fgcolor(),
            Color::Idx(2)
        );
        assert_eq!(screen.cell(0, 11).expect("right pane cell").contents(), "B");
        let rendered_again = state
            .render_session(session_id, None)
            .expect("render split session again");
        assert_eq!(
            rendered, rendered_again,
            "identical pane state must produce an identical frame"
        );
        let incremental = state
            .render_session_with_clear(session_id, None, false)
            .expect("render incremental split session");
        assert!(
            !incremental
                .windows(b"\x1b[2J".len())
                .any(|window| window == b"\x1b[2J"),
            "incremental frames must not clear the entire terminal"
        );
        let before_incremental = terminal.screen().clone();
        terminal.process(&incremental);
        let after_incremental = terminal.screen().clone();
        assert!(
            matches!(
                render_screen_delta(&before_incremental, &after_incremental),
                Ok(None)
            ),
            "an unchanged pane state must have no screen delta"
        );
    }

    #[test]
    fn rendered_status_line_preserves_active_pane_cursor_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("render-cursor"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create render cursor session");
        let pane = state.sessions[0].windows[0].panes[0].id;
        state
            .find_pane_mut(pane)
            .expect("render cursor pane")
            .parser
            .process(b"shell$ ");

        let session_id = state.sessions[0].id;
        let rendered = state
            .render_session(session_id, None)
            .expect("render cursor session");
        let mut terminal = Parser::new(6, 20, 100);
        terminal.process(&rendered);

        assert_eq!(
            terminal.screen().cursor_position(),
            (0, 7),
            "status-line output must not steal the shell cursor"
        );
        assert!(
            !terminal.screen().hide_cursor(),
            "the active pane cursor must remain visible"
        );
    }

    #[test]
    fn incremental_render_hides_cursor_before_changed_rows_headlessly() {
        let mut previous = Parser::new(2, 20, 0);
        previous.process(b"old");
        let mut current = Parser::new(2, 20, 0);
        current.process(b"new");

        let delta = render_screen_delta(previous.screen(), current.screen())
            .expect("compatible terminal states")
            .expect("changed row delta");
        assert!(
            delta.starts_with(b"\x1b[?25l"),
            "incremental redraw must hide a visible cursor before painting rows"
        );

        previous.process(b"\x1b[?25l");
        current.process(b"\x1b[?25l");
        let hidden_delta = render_screen_delta(previous.screen(), current.screen())
            .expect("compatible hidden terminal states")
            .expect("changed hidden row delta");
        assert!(
            !hidden_delta
                .windows(b"\x1b[?25l".len())
                .any(|window| window == b"\x1b[?25l"),
            "incremental redraw must not repeat an already-hidden cursor"
        );
    }

    #[test]
    fn idle_attached_delta_is_empty_headlessly() {
        let mut fixture = RenderBenchmark::new(80, 24, 1);
        let initial = fixture.render_delta_frame();
        let idle = fixture.render_delta_frame();
        assert!(
            idle.is_empty(),
            "idle attached delta had {} bytes after a {} byte initial frame: {:?}",
            idle.len(),
            initial.len(),
            String::from_utf8_lossy(&idle)
        );
    }

    #[test]
    fn sgr_mouse_drag_resizes_the_grabbed_pane_separator_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("mouse-resize"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create mouse resize session");
        state
            .split_window(
                &shared,
                Some("mouse-resize:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split mouse resize session");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse resize events");
        let session_id = state.sessions[0].id;
        let (_, client_id) = state
            .register_client(Some("mouse-resize"), Size::new(20, 6))
            .expect("register mouse resize client");
        state.handle_client_input(client_id, b"\x1b[<0;11;2M", &shared);
        state.handle_client_input(client_id, b"\x1b[<32;14;2M", &shared);
        state.handle_client_input(client_id, b"\x1b[<0;14;2m", &shared);
        let window = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.active_window())
            .expect("mouse resize window");
        assert_eq!(window.panes[0].rect.cols, 13);
        assert_eq!(window.panes[1].rect.x, 14);
    }

    #[test]
    fn application_mouse_mode_gets_local_pane_coordinates_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("mouse-pass-through"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 6),
            )
            .expect("create mouse pass-through session");
        state
            .split_window(
                &shared,
                Some("mouse-pass-through:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split mouse pass-through session");
        let right_pane = state.sessions[0].windows[0].panes[1].id;
        state
            .find_pane_mut(right_pane)
            .expect("right mouse pane")
            .parser
            .process(b"\x1b[?1000h\x1b[?1006h");
        let session_id = state.sessions[0].id;
        assert_eq!(
            state.mouse_passthrough_target(session_id, 25, 2),
            Some((
                right_pane,
                4,
                2,
                vt100::MouseProtocolEncoding::Sgr,
            ))
        );
        assert_eq!(encode_sgr_mouse(0, 4, 2, false), b"\x1b[<0;4;2M");
    }

    #[test]
    fn exited_pane_reflows_the_surviving_panes_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("exited-reflow"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 6),
            )
            .expect("create exited reflow session");
        state
            .split_window(
                &shared,
                Some("exited-reflow:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split exited reflow session");
        let removed = state.sessions[0].windows[0].panes[1].id;
        state.remove_exited_panes(&std::collections::HashSet::from([removed]));
        let window = &state.sessions[0].windows[0];
        assert_eq!(window.panes.len(), 1);
        assert_eq!(window.panes[0].rect.x, 0);
        assert_eq!(window.panes[0].rect.cols, 40);
        assert_eq!(window.panes[0].rect.rows, 6);
    }

    #[test]
    fn mode_kill_flags_remove_the_pane_that_entered_each_mode_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("mode-kill"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create mode-kill session");
        state.store_buffer(Some("kill-buffer".to_owned()), b"buffer".to_vec(), false);
        let (_, client_id) = state
            .register_client(Some("mode-kill"), Size::new(40, 8))
            .expect("register mode-kill client");

        for mode in ["tree", "buffer", "client", "panes"] {
            if state.sessions[0].windows[0].panes.len() == 1 {
                state
                    .split_window(
                        &shared,
                        Some("mode-kill:0"),
                        true,
                        false,
                        false,
                        false,
                        false,
                        true,
                        None,
                        &[],
                        None,
                    )
                    .expect("split mode-kill pane");
            }
            let pane_id = state.sessions[0].windows[0].active_pane;
            let _ = match mode {
                "tree" => state
                    .enter_tree_mode(
                        client_id,
                        Some("mode-kill:0"),
                        None,
                        None,
                        "index",
                        false,
                        false,
                        true,
                    )
                    .expect("enter kill tree mode"),
                "buffer" => state
                    .enter_buffer_mode(
                        client_id,
                        Some("mode-kill:0"),
                        None,
                        None,
                        "index",
                        false,
                        true,
                    )
                    .expect("enter kill buffer mode"),
                "client" => state
                    .enter_client_mode(client_id, Some("mode-kill:0"), None, None, true)
                    .expect("enter kill client mode"),
                "panes" => state
                    .enter_panes_mode(client_id, Some("mode-kill:0"), None, true, None, true)
                    .expect("enter kill panes mode"),
                _ => unreachable!(),
            };
            state.handle_client_input(client_id, b"q", &shared);
            assert!(
                state.find_pane(pane_id).is_none(),
                "{mode} -k left the entering pane alive"
            );
        }
    }

    #[test]
    fn synchronized_panes_receive_active_input_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let left = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "read line; printf 'left:%s\\n' \"$line\"; sleep 30".to_owned(),
        ];
        let right = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "read line; printf 'right:%s\\n' \"$line\"; sleep 30".to_owned(),
        ];
        {
            let mut state = shared.lock().expect("server state lock");
            state
                .create_session(
                    &shared,
                    Some("sync"),
                    false,
                    None,
                    None,
                    None,
                    false,
                    &left,
                    None,
                    Size::new(40, 6),
                )
                .expect("create sync session");
            state
                .split_window(
                    &shared,
                    Some("sync:0"),
                    true,
                    false,
                    false,
                    false,
                    false,
                    false,
                    None,
                    &right,
                    None,
                )
                .expect("split sync window");
            state
                .set_window_option(Some("sync:0"), "synchronize-panes", "on")
                .expect("enable synchronized panes");
            let session_id = state.sessions[0].id;
            state.write_active(session_id, b"hello\r");
        }
        thread::sleep(Duration::from_millis(150));
        let mut state = shared.lock().expect("server state lock");
        let left_capture = state
            .capture_pane(Some("sync:0.0"), None, None, false, false, false)
            .expect("capture left pane");
        let right_capture = state
            .capture_pane(Some("sync:0.1"), None, None, false, false, false)
            .expect("capture right pane");
        assert!(
            left_capture.contains("left:hello"),
            "left: {left_capture:?}"
        );
        assert!(
            right_capture.contains("right:hello"),
            "right: {right_capture:?}"
        );
    }

    #[test]
    fn attached_client_registry_switches_and_detaches_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("one"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(80, 24),
            )
            .expect("create first session");
        state
            .create_session(
                &shared,
                Some("two"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(80, 24),
            )
            .expect("create second session");
        let (_, client_id) = state
            .register_client(Some("one"), Size::new(100, 40))
            .expect("register client");
        assert_eq!(
            state
                .list_clients(Some(
                    "#{client_name}:#{client_session}:#{client_width}x#{client_height}"
                ))
                .expect("list clients"),
            format!("client{client_id}:one:100x40")
        );
        state
            .switch_client(Some(&format!("client{client_id}")), "two")
            .expect("switch client");
        assert!(
            state
                .list_clients(Some("#{client_session}"))
                .expect("list switched client")
                .contains("two")
        );
        state
            .detach_client(Some(&format!("client{client_id}")), false)
            .expect("detach client");
        assert!(state.clients.is_empty());
    }

    #[test]
    fn copy_mode_render_uses_scrollback_view_and_copy_cursor_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option(
                "copy-mode-position-format",
                "#[align=right][23/100-LONGTAIL]",
                false,
            )
            .expect("set copy position format");
        state
            .set_global_option("copy-mode-line-numbers", "on", false)
            .expect("enable copy line numbers");
        state
            .create_session(
                &shared,
                Some("render-copy"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 3),
            )
            .expect("create render session");
        let session_id = state.sessions[0].id;
        let pane = state.sessions[0].windows[0]
            .panes
            .first_mut()
            .expect("render pane");
        pane.parser.process(b"one\ntwo\nthree\nfour\n");
        pane.raw_output = b"one\ntwo\nthree\nfour\n".to_vec();
        state
            .enter_copy_mode(Some("render-copy"), None, false, false, false, false)
            .expect("enter copy mode");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("history top");
        let rendered = state
            .render_session(session_id, None)
            .expect("render copy mode");
        assert!(rendered.windows(8).any(|window| window == b"o\x1b[27mne"));
        assert!(rendered.windows(4).any(|window| window == b"\x1b[7m"));
        assert!(
            rendered
                .windows(b"[23/100-LONGTAIL]".len())
                .any(|window| window == b"[23/100-LONGTAIL]")
        );
        assert!(
            rendered
                .windows(b"\x1b[2m  1 \x1b[0m".len())
                .any(|window| window == b"\x1b[2m  1 \x1b[0m")
        );
        state
            .set_global_option("copy-mode-position-format", "#[align=right][1/100]", false)
            .expect("shrink copy position format");
        let rendered = state
            .render_session(session_id, None)
            .expect("rerender copy mode");
        assert!(
            rendered
                .windows(b"[1/100]".len())
                .any(|window| window == b"[1/100]")
        );
        assert!(
            !rendered
                .windows(b"LONGTAIL".len())
                .any(|window| window == b"LONGTAIL")
        );
    }

    #[test]
    fn copy_mode_redraw_clears_long_tail_before_a_short_tabbed_row_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("status", "off", false)
            .expect("hide status for redraw test");
        state
            .create_session(
                &shared,
                Some("redraw-tail"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create redraw session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = concat!(
            "LONGTAIL-ABCDEFGHIJKLMNOP\r\n",
            "A\tB\r\n",
            "LONGTAIL-1234567890123456\r\n",
            "S\r\n",
            "FILLER-00\r\n",
            "FILLER-01\r\n",
            "FILLER-02\r\n",
            "FILLER-03\r\n",
            "FILLER-04\r\n",
            "FILLER-05\r\n",
        );
        let pane = state.find_pane_mut(pane_id).expect("redraw pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.as_bytes().to_vec();
        let session_id = state.sessions[0].id;
        state
            .enter_copy_mode(Some("redraw-tail"), None, false, true, false, false)
            .expect("enter redraw copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move redraw view to history top");
        let rendered = state
            .render_session(session_id, None)
            .expect("render long redraw row");
        let mut terminal = Parser::new(5, 30, 10_000);
        terminal.process(&rendered);
        assert!(
            terminal
                .screen()
                .contents()
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("LONGTAIL-"))
        );

        state
            .execute_copy_action(pane_id, CopyAction::ScrollDown, 1)
            .expect("move redraw view to short row");
        let rendered = state
            .render_session(session_id, None)
            .expect("render short redraw row");
        let mut terminal = Parser::new(5, 30, 10_000);
        terminal.process(&rendered);
        let first_row = terminal
            .screen()
            .contents()
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        assert!(
            first_row.starts_with("A       B"),
            "unexpected short row: {first_row:?}"
        );
        assert!(!first_row.contains("LONGTAIL"));
    }

    #[test]
    fn copy_mode_source_pane_renders_the_source_view_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("render-source"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create source render session");
        state
            .split_window(
                &shared,
                Some("render-source:0"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split source render session");
        let session_id = state.sessions[0].id;
        let panes = &mut state.sessions[0].windows[0].panes;
        let source_id = panes[0].id;
        let target_id = panes[1].id;
        panes[0].parser.process(b"SOURCE LINE\n");
        panes[0].raw_output = b"SOURCE LINE\n".to_vec();
        panes[1].parser.process(b"TARGET LINE\n");
        panes[1].raw_output = b"TARGET LINE\n".to_vec();

        state
            .enter_copy_mode(
                Some(&format!("%{target_id}")),
                Some(&format!("%{source_id}")),
                false,
                true,
                false,
                false,
            )
            .expect("enter source copy mode");
        let rendered = state
            .render_session(session_id, None)
            .expect("render source copy mode");
        let source_occurrences = rendered
            .windows(b"SOURCE LINE".len())
            .filter(|window| *window == b"SOURCE LINE")
            .count();
        assert!(source_occurrences >= 2, "{rendered:?}");
        assert!(
            !rendered
                .windows(b"TARGET LINE".len())
                .any(|window| window == b"TARGET LINE")
        );
    }

    #[test]
    fn copy_mode_format_context_exposes_cursor_word_and_line_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-format"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create copy format session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy format pane");
        let output = b"\x1b]8;;https://example.com\x1b\\alpha beta\x1b]8;;\x1b\\\nsecond line\n";
        pane.parser.process(output);
        pane.raw_output = output.to_vec();
        state
            .enter_copy_mode(Some("copy-format"), None, false, false, false, false)
            .expect("enter copy format mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move copy cursor to history top");

        assert_eq!(
            state
                .display_message(
                    Some("copy-format"),
                    "#{copy_cursor_word}|#{copy_cursor_line}|#{copy_cursor_x},#{copy_cursor_y}|#{copy_cursor_hyperlink}"
                )
                .expect("render copy format context"),
            "alpha|alpha beta|0,0|https://example.com"
        );
    }

    #[test]
    fn copy_mode_line_number_modes_render_absolute_relative_and_hybrid_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-lines"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create line number session");
        let session_id = state.sessions[0].id;
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("line number pane");
        pane.parser.process(b"zero\none\ntwo\nthree\n");
        pane.raw_output = b"zero\none\ntwo\nthree\n".to_vec();

        state
            .set_global_option("copy-mode-line-numbers", "absolute", false)
            .expect("set absolute line numbers");
        state
            .enter_copy_mode(Some("copy-lines"), None, false, false, false, false)
            .expect("enter absolute copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move to absolute history top");
        let rendered = state
            .render_session(session_id, None)
            .expect("render absolute line numbers");
        assert!(
            rendered
                .windows(b"\x1b[2m  1 \x1b[0m".len())
                .any(|window| window == b"\x1b[2m  1 \x1b[0m")
        );

        state
            .set_global_option("copy-mode-line-numbers", "relative", false)
            .expect("set relative line numbers");
        state
            .enter_copy_mode(Some("copy-lines"), None, false, false, false, false)
            .expect("enter relative copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("reset relative history top");
        state
            .execute_copy_action(pane_id, CopyAction::CursorDown, 1)
            .expect("move relative cursor");
        let rendered = state
            .render_session(session_id, None)
            .expect("render relative line numbers");
        assert!(
            rendered
                .windows(b"\x1b[2m  0 \x1b[0m".len())
                .any(|window| window == b"\x1b[2m  0 \x1b[0m")
        );

        state
            .set_global_option("copy-mode-line-numbers", "hybrid", false)
            .expect("set hybrid line numbers");
        state
            .enter_copy_mode(Some("copy-lines"), None, false, false, false, false)
            .expect("enter hybrid copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("reset hybrid history top");
        state
            .execute_copy_action(pane_id, CopyAction::CursorDown, 1)
            .expect("move hybrid cursor");
        let rendered = state
            .render_session(session_id, None)
            .expect("render hybrid line numbers");
        assert!(
            rendered
                .windows(b"\x1b[2m  2 \x1b[0m".len())
                .any(|window| window == b"\x1b[2m  2 \x1b[0m")
        );
    }

    #[test]
    fn configured_split_binding_expands_the_current_path_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let cwd = std::env::current_dir().expect("current directory");
        let cwd = cwd.to_string_lossy().into_owned();
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("binding"),
                false,
                None,
                None,
                None,
                true,
                &[],
                Some(&cwd),
                Size::new(40, 8),
            )
            .expect("create binding session");
        let (session_id, client_id) = state
            .register_client(Some("binding"), Size::new(40, 8))
            .expect("register binding client");
        let line = config::parse(
            r###"bind - split-window -h -c "#{pane_current_path}" \; select-layout even-horizontal"###,
        )
        .into_iter()
        .next()
        .expect("parse split binding");
        state
            .execute_config_line(0, line, &shared)
            .expect("install split binding");
        let binding = state
            .bindings
            .get(&vec![b'-'])
            .cloned()
            .expect("literal dash binding");
        state
            .execute_bound_commands(client_id, session_id, binding, &shared)
            .expect("execute configured split");
        assert_eq!(state.sessions[0].windows[0].panes.len(), 2);
    }

    #[test]
    fn copy_selection_emits_external_clipboard_data_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("set-clipboard", "external", false)
            .expect("enable external clipboard");
        state
            .create_session(
                &shared,
                Some("clipboard"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create clipboard session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("clipboard pane");
        pane.parser.process(b"copy me\n");
        pane.raw_output = b"copy me\n".to_vec();
        state
            .enter_copy_mode(Some("clipboard"), None, false, false, false, false)
            .expect("enter clipboard copy mode");
        for action in [
            CopyAction::HistoryTop,
            CopyAction::StartOfLine,
            CopyAction::BeginSelection,
            CopyAction::EndOfLine,
            CopyAction::CopySelection,
        ] {
            state
                .execute_copy_action(pane_id, action, 1)
                .expect("execute clipboard action");
        }
        let session_id = state.sessions[0].id;
        let rendered = state
            .render_session(session_id, None)
            .expect("render clipboard event");
        assert!(
            rendered
                .windows(b"\x1b]52;c;Y29weSBtZQ==\x07".len())
                .any(|window| window == b"\x1b]52;c;Y29weSBtZQ==\x07")
        );
        let next_render = state
            .render_session(session_id, None)
            .expect("render after clipboard event");
        assert!(
            !next_render
                .windows(b"Y29weSBtZQ==".len())
                .any(|window| window == b"Y29weSBtZQ==")
        );
    }

    #[test]
    fn copy_selection_clipboard_flag_suppresses_external_data_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("set-clipboard", "external", false)
            .expect("enable external clipboard");
        state
            .create_session(
                &shared,
                Some("clipboard-flag"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create clipboard flag session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("clipboard flag pane");
        pane.parser.process(b"copy me without clipboard\n");
        pane.raw_output = b"copy me without clipboard\n".to_vec();
        state
            .enter_copy_mode(Some("clipboard-flag"), None, false, false, false, false)
            .expect("enter clipboard flag copy mode");
        for action in [
            CopyAction::HistoryTop,
            CopyAction::StartOfLine,
            CopyAction::BeginSelection,
            CopyAction::EndOfLine,
        ] {
            state
                .execute_copy_action(pane_id, action, 1)
                .expect("execute clipboard flag action");
        }
        state
            .execute_copy_action(
                pane_id,
                parse_copy_action("copy-selection\x1dC").expect("parse clipboard flag"),
                1,
            )
            .expect("execute clipboard flag selection");

        assert_eq!(
            state.show_buffer(None).expect("show copied buffer"),
            "copy me without clipboard"
        );
        assert!(state.clipboard_pending.is_none());
    }

    #[test]
    fn copy_mode_refresh_actions_track_active_state_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-refresh"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create copy refresh session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy refresh pane");
        pane.parser.process(b"refresh me\n");
        pane.raw_output = b"refresh me\n".to_vec();
        state
            .enter_copy_mode(Some("copy-refresh"), None, false, false, false, false)
            .expect("enter copy refresh mode");

        assert_eq!(
            state
                .display_message(Some("copy-refresh"), "#{refresh_active}")
                .expect("read initial refresh state"),
            "0"
        );
        state
            .execute_copy_action(pane_id, CopyAction::RefreshOn, 1)
            .expect("enable copy refresh");
        assert_eq!(
            state
                .display_message(Some("copy-refresh"), "#{refresh_active}")
                .expect("read enabled refresh state"),
            "1"
        );
        state
            .execute_copy_action(pane_id, CopyAction::RefreshToggle, 1)
            .expect("toggle copy refresh");
        assert_eq!(
            state
                .display_message(Some("copy-refresh"), "#{refresh_active}")
                .expect("read toggled refresh state"),
            "0"
        );
        state
            .execute_copy_action(pane_id, CopyAction::RefreshOff, 1)
            .expect("disable copy refresh");
        assert_eq!(
            state
                .display_message(Some("copy-refresh"), "#{refresh_active}")
                .expect("read disabled refresh state"),
            "0"
        );
    }

    #[test]
    fn copy_mode_refresh_now_follows_new_live_output_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-refresh-now"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create refresh-now session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("refresh-now pane");
        pane.parser.process(b"first\nsecond\n");
        pane.raw_output = b"first\nsecond\n".to_vec();
        state
            .enter_copy_mode(Some("copy-refresh-now"), None, false, false, false, false)
            .expect("enter refresh-now copy mode");
        let before = state
            .display_message(Some("copy-refresh-now"), "#{copy_cursor_y}")
            .expect("read cursor before refresh")
            .parse::<usize>()
            .expect("parse cursor before refresh");

        let pane = state
            .find_pane_mut(pane_id)
            .expect("refresh-from-pane output pane");
        pane.parser.process(b"third\n");
        pane.raw_output.extend_from_slice(b"third\n");
        state
            .execute_copy_action(pane_id, CopyAction::RefreshFromPane, 1)
            .expect("refresh live output");
        let after = state
            .display_message(Some("copy-refresh-now"), "#{copy_cursor_y}")
            .expect("read cursor after refresh")
            .parse::<usize>()
            .expect("parse cursor after refresh");
        assert!(after > before, "refresh-now did not follow new output");
    }

    #[test]
    fn compiled_interactive_binding_vocabulary_executes_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_compiled_interactive_config();

        assert_eq!(state.prefix, vec![1]);
        assert_eq!(state.history_limit, 10_000);
        assert_eq!(state.global_base_index(), 1);
        assert_eq!(state.global_options.get("mouse"), Some(&"on".to_owned()));
        assert_eq!(
            state.global_options.get("focus-events"),
            Some(&"on".to_owned())
        );
        assert_eq!(
            state.global_options.get("extended-keys"),
            Some(&"on".to_owned())
        );
        assert_eq!(
            state.global_options.get("set-clipboard"),
            Some(&"external".to_owned())
        );
        for key in [
            "Enter", "C-s", "/", "r", "n", "C-Left", "C-Right", "Left", "Right", "\\", "-", "z",
            "k", "p", "m", "C-n",
        ] {
            assert!(
                state
                    .bindings
                    .contains_key(&config::key_bytes(key).expect("config key")),
                "missing configured binding {key}"
            );
        }
        assert_eq!(
            state.global_options.get("status-left"),
            Some(&"#{?client_prefix,#[fg=yellow],}(#S) ".to_owned())
        );

        state
            .create_session(
                &shared,
                Some("configured-keys"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create configured-key session");
        let (_, client_id) = state
            .register_client(Some("configured-keys"), Size::new(40, 8))
            .expect("register configured-key client");
        let session_id = state.clients[&client_id].session_id;

        state.handle_client_input(client_id, b"\x01r", &shared);
        state.handle_client_input(client_id, b"renamed\r", &shared);
        assert_eq!(state.sessions[0].windows[0].name, "renamed");

        state.handle_client_input(client_id, b"\x01n", &shared);
        state.handle_client_input(client_id, b"new\r", &shared);
        assert_eq!(state.sessions[0].windows.len(), 2);
        assert_eq!(state.sessions[0].active_window, 2);
        assert_eq!(state.sessions[0].windows[1].name, "new");

        state.handle_client_input(client_id, b"\x01\x1b[1;5D", &shared);
        assert_eq!(state.sessions[0].active_window, 1);
        state.handle_client_input(client_id, b"\x01\x1b[1;5C", &shared);
        assert_eq!(state.sessions[0].active_window, 2);
        state.handle_client_input(client_id, b"\x01\x1b[1;5D", &shared);
        assert_eq!(state.sessions[0].active_window, 1);

        state.handle_client_input(client_id, b"\x01z", &shared);
        assert!(state.sessions[0].windows[0].zoomed);
        let zoom_render = state
            .render_session(session_id, Some(client_id))
            .expect("render configured zoom status");
        assert!(
            zoom_render
                .windows(b"(Z)".len())
                .any(|window| window == b"(Z)")
        );
        state.handle_client_input(client_id, b"\x01z", &shared);
        assert!(!state.sessions[0].windows[0].zoomed);

        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render configured status");
        assert!(
            rendered
                .windows(b"(configured-keys)".len())
                .any(|window| window == b"(configured-keys)"),
            "configured status was not rendered: {:?}",
            String::from_utf8_lossy(&rendered)
        );
        state.handle_client_input(client_id, b"\x01", &shared);
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render configured prefix status");
        assert!(
            rendered
                .windows(b"\x1b[33m(configured-keys)".len())
                .any(|window| { window == b"\x1b[33m(configured-keys)" })
        );
        state
            .clients
            .get_mut(&client_id)
            .expect("configured client")
            .prefix_pending = false;
        state.handle_client_input(client_id, b"\x01\\", &shared);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 2);
        let active_after_split = state.sessions[0].windows[0].active_pane;
        let first_pane = state.sessions[0].windows[0].panes[0].id;
        state.handle_client_input(client_id, b"\x01p", &shared);
        assert_eq!(state.sessions[0].windows[0].active_pane, active_after_split);
        assert!(state.clients[&client_id].panes_mode.is_some());
        state.handle_client_input(client_id, b"0", &shared);
        assert_eq!(state.sessions[0].windows[0].active_pane, first_pane);
        assert!(state.clients[&client_id].panes_mode.is_none());
        state.handle_client_input(client_id, b"\x01k", &shared);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 1);
    }

    #[test]
    fn pane_move_break_and_swap_bindings_execute_headlessly() {
        let config = r###"set -g prefix C-a
set -g base-index 1
set -g renumber-windows on
bind - split-window -v \; select-layout even-vertical
bind m command-prompt -p "move pane to window #:" "join-pane -h -t '%%'"
bind -r C-n break-pane -t :
bind -r Left swap-window -t -1\; select-window -t -1
bind -r Right swap-window -t +1\; select-window -t +1
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("pane-bindings"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create pane binding session");
        let (_, client_id) = state
            .register_client(Some("pane-bindings"), Size::new(40, 8))
            .expect("register pane binding client");

        state.handle_client_input(client_id, b"\x01-", &shared);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 2);

        state
            .new_window(
                &shared,
                Some("pane-bindings"),
                Some("destination"),
                false,
                None,
                false,
                None,
                false,
                false,
                false,
                false,
                &[],
                None,
            )
            .expect("create destination window");
        assert_eq!(state.sessions[0].active_window, 2);

        state.handle_client_input(client_id, b"\x01m", &shared);
        state.handle_client_input(client_id, b"1\r", &shared);
        assert_eq!(state.sessions[0].windows.len(), 1);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 3);

        state.handle_client_input(client_id, b"\x01\x0e", &shared);
        assert_eq!(state.sessions[0].windows.len(), 2);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 2);
        let broken_window = state.sessions[0].active_window;
        assert_ne!(broken_window, 1);

        state.handle_client_input(client_id, b"\x01\x1b[D", &shared);
        assert_eq!(state.sessions[0].active_window, 1);
        state.handle_client_input(client_id, b"\x01\x1b[C", &shared);
        assert_eq!(state.sessions[0].active_window, broken_window);
    }

    #[test]
    fn configured_prefix_and_window_prompt_drive_the_bound_command_headlessly() {
        let config = r###"set -g prefix C-a
set -g base-index 1
bind n command-prompt -p "name of new window:" "new-window -n '%%'"
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("prompt"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prompt session");
        let (session_id, client_id) = state
            .register_client(Some("prompt"), Size::new(40, 8))
            .expect("register prompt client");
        state.handle_client_input(client_id, b"\x01n", &shared);
        assert!(state.clients[&client_id].prompt.is_some());
        let prompt_render = state
            .render_session(session_id, Some(client_id))
            .expect("render prompt");
        assert!(
            prompt_render
                .windows(b"name of new window:".len())
                .any(|window| window == b"name of new window:")
        );
        let mut prompt_terminal = Parser::new(8, 40, 0);
        prompt_terminal.process(&prompt_render);
        assert_eq!(
            prompt_terminal.screen().cursor_position(),
            (7, 19),
            "status command prompts own the client cursor"
        );
        assert!(!prompt_terminal.screen().hide_cursor());
        state.handle_client_input(client_id, b"renamed\r", &shared);
        assert_eq!(
            state
                .list_windows(Some("prompt"), Some("#{window_index}:#{window_name}"))
                .expect("list prompted windows"),
            "1:0\n2:renamed"
        );
    }

    #[test]
    fn prefix_digits_select_windows_by_index_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("prefix-digits"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prefix digit session");
        state
            .new_window(
                &shared,
                Some("prefix-digits"),
                Some("second"),
                false,
                None,
                false,
                None,
                false,
                false,
                false,
                true,
                &[],
                None,
            )
            .expect("create second prefix digit window");
        let (_, client_id) = state
            .register_client(Some("prefix-digits"), Size::new(40, 8))
            .expect("register prefix digit client");
        state.handle_client_input(client_id, b"\x020", &shared);
        assert_eq!(state.sessions[0].active_window, 0);
        state.handle_client_input(client_id, b"\x021", &shared);
        assert_eq!(state.sessions[0].active_window, 1);
    }

    #[test]
    fn three_vertical_panes_keep_each_separator_visible_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_compiled_interactive_config();
        state
            .create_session(
                &shared,
                Some("three-vertical"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create three vertical session");
        let (session_id, client_id) = state
            .register_client(Some("three-vertical"), Size::new(40, 8))
            .expect("register three vertical client");
        state.handle_client_input(client_id, b"\x01\\", &shared);
        state.handle_client_input(client_id, b"\x01\\", &shared);
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render three vertical panes");
        let mut terminal = Parser::new(8, 40, 0);
        terminal.process(&rendered);
        assert_eq!(terminal.screen().cell(0, 20).map(|cell| cell.contents()), Some("│"));
        assert_eq!(terminal.screen().cell(0, 30).map(|cell| cell.contents()), Some("│"));
    }

    #[test]
    fn command_prompt_supports_cursor_editing_headlessly() {
        let config = r###"set -g prefix C-a
bind n command-prompt -p "name: " "rename-window '%%'"
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("prompt-edit"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prompt editing session");
        let (_, client_id) = state
            .register_client(Some("prompt-edit"), Size::new(40, 8))
            .expect("register prompt editing client");
        state.handle_client_input(client_id, b"\x01n", &shared);
        state.handle_client_input(
            client_id,
            b"abcd\x1b[D\x1b[DXY\x7f\x1b[1~Y\x1b[4~Z\r",
            &shared,
        );
        assert_eq!(
            state
                .list_windows(Some("prompt-edit"), Some("#{window_name}"))
                .expect("list edited window"),
            "YabXcdZ"
        );
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| &prompt.input),
            None
        );
    }

    #[test]
    fn command_prompt_recalls_history_with_up_headlessly() {
        let config = r###"set -g prefix C-a
bind / command-prompt
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("prompt-history"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prompt history session");
        let (_, client_id) = state
            .register_client(Some("prompt-history"), Size::new(40, 8))
            .expect("register prompt history client");

        state.handle_client_input(client_id, b"\x01/display first\r", &shared);
        assert_eq!(state.last_message.as_deref(), Some("first"));
        state.handle_client_input(client_id, b"\x01/\x1b[A", &shared);

        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| String::from_utf8_lossy(&prompt.input).into_owned()),
            Some("display first".to_owned())
        );
    }

    #[test]
    fn command_prompt_mechanics_match_tmux_headlessly() {
        let config = r###"set -g prefix C-a
bind s command-prompt -1 -p "one: " "set -g @single '%%'"
bind n command-prompt -N -I 5 -p "num: " "set -g @numeric '%%'"
bind k command-prompt -k -p "key: " "set -g @key '%%'"
bind e command-prompt -e -p "backspace: " "set -g @bspace '%%'"
bind i command-prompt -I hello -p "prefill: " "set -g @prefill '%%'"
bind x command-prompt -i -p "incremental: " "set -g @incremental '%%'"
bind m command-prompt -p "first,second" "set -g @multi '%1/%2'"
bind p command-prompt -P -p "pane: " "set -g @pane '%%'"
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("prompt-mechanics"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prompt mechanics session");
        let (_, client_id) = state
            .register_client(Some("prompt-mechanics"), Size::new(40, 8))
            .expect("register prompt mechanics client");

        state.handle_client_input(client_id, b"\x01sq", &shared);
        assert_eq!(state.global_options.get("@single"), Some(&"q".to_owned()));

        state.handle_client_input(client_id, b"\x01n7\r", &shared);
        assert_eq!(state.global_options.get("@numeric"), Some(&"57".to_owned()));
        state.handle_client_input(client_id, b"\x01nx", &shared);
        assert_eq!(state.global_options.get("@numeric"), Some(&"5".to_owned()));

        state.handle_client_input(client_id, b"\x01kz", &shared);
        assert_eq!(state.global_options.get("@key"), Some(&"z".to_owned()));

        state
            .set_global_option("@bspace", "SENTINEL", false)
            .expect("set bspace sentinel");
        state.handle_client_input(client_id, b"\x01e\x7f", &shared);
        assert!(state.clients[&client_id].prompt.is_none());
        assert_eq!(
            state.global_options.get("@bspace"),
            Some(&"SENTINEL".to_owned())
        );

        state.handle_client_input(client_id, b"\x01i\r", &shared);
        assert_eq!(
            state.global_options.get("@prefill"),
            Some(&"hello".to_owned())
        );

        state.handle_client_input(client_id, b"\x01x", &shared);
        assert_eq!(
            state.global_options.get("@incremental"),
            Some(&"=".to_owned())
        );
        state.handle_client_input(client_id, b"ab", &shared);
        assert_eq!(
            state.global_options.get("@incremental"),
            Some(&"=ab".to_owned())
        );
        assert!(state.clients[&client_id].prompt.is_some());
        state.handle_client_input(client_id, b"\x1b", &shared);

        state.handle_client_input(client_id, b"\x01mX\r", &shared);
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| prompt.label.as_str()),
            Some("second")
        );
        state.handle_client_input(client_id, b"Y\r", &shared);
        assert_eq!(state.global_options.get("@multi"), Some(&"X/Y".to_owned()));

        state.handle_client_input(client_id, b"\x01p", &shared);
        let session_id = state.clients[&client_id].session_id;
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render pane prompt");
        let mut terminal = Parser::new(8, 40, 0);
        terminal.process(&rendered);
        let pane_row = terminal.screen().rows(0, 40).nth(6).unwrap_or_default();
        let status_row = terminal.screen().rows(0, 40).nth(7).unwrap_or_default();
        assert!(
            pane_row.contains("pane: "),
            "pane prompt row: {pane_row:?}; status row: {status_row:?}"
        );
        assert!(
            !status_row.contains("pane: "),
            "status prompt row: {status_row:?}"
        );
        assert_eq!(
            terminal.screen().cursor_position(),
            (6, 6),
            "pane prompts own the client cursor"
        );
        state.handle_client_input(client_id, b"z\r", &shared);
        assert_eq!(state.global_options.get("@pane"), Some(&"z".to_owned()));
    }

    #[test]
    fn command_prompt_history_completion_and_reentry_match_tmux_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("prompt-details"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create prompt details session");
        let (_, client_id) = state
            .register_client(Some("prompt-details"), Size::new(40, 8))
            .expect("register prompt details client");

        let command_prompt = |prompt_type: Option<&str>, template: &str| ConfigBinding {
            _repeat: false,
            commands: vec![{
                let mut command = vec!["command-prompt".to_owned()];
                if let Some(prompt_type) = prompt_type {
                    command.extend(["-T".to_owned(), prompt_type.to_owned()]);
                }
                command.extend(["-p".to_owned(), "> ".to_owned(), template.to_owned()]);
                command
            }],
        };

        let session_id = state.clients[&client_id].session_id;
        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(None, "set -g @command '%%'"),
                &shared,
            )
            .expect("open command prompt");
        state.handle_client_input(client_id, b"alpha\r", &shared);
        assert_eq!(
            state.global_options.get("@command"),
            Some(&"alpha".to_owned())
        );

        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(None, "set -g @command '%%'"),
                &shared,
            )
            .expect("reopen command prompt");
        state.handle_client_input(client_id, b"\x1b[A", &shared);
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| String::from_utf8_lossy(&prompt.input).into_owned()),
            Some("alpha".to_owned())
        );
        state.handle_client_input(client_id, b"\x1b", &shared);

        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(Some("search"), "set -g @search '%%'"),
                &shared,
            )
            .expect("open search prompt");
        state.handle_client_input(client_id, b"\x1b[A", &shared);
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| String::from_utf8_lossy(&prompt.input).into_owned()),
            Some(String::new())
        );
        state.handle_client_input(client_id, b"\x1b", &shared);

        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(None, "set -g @command '%%'"),
                &shared,
            )
            .expect("open completion prompt");
        state.handle_client_input(client_id, b"new-w\t", &shared);
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| String::from_utf8_lossy(&prompt.input).into_owned()),
            Some("new-window".to_owned())
        );
        state.handle_client_input(client_id, b"\x1b", &shared);

        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(None, "set -g @outer '%%'"),
                &shared,
            )
            .expect("open outer prompt");
        state
            .execute_bound_commands(
                client_id,
                session_id,
                command_prompt(Some("nested"), "set -g @nested '%%'"),
                &shared,
            )
            .expect("refuse nested prompt");
        assert_eq!(
            state.clients[&client_id]
                .prompt
                .as_ref()
                .map(|prompt| prompt.label.as_str()),
            Some("> ")
        );
        state.handle_client_input(client_id, b"\x1b", &shared);
    }

    #[test]
    fn copy_mode_key_table_bindings_override_default_keys_headlessly() {
        let config = r###"set -g prefix C-a
bind -T copy-mode C-a send -X end-of-line
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("copy-key-table"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create copy key table session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy key table pane");
        pane.parser.process(b"alpha\n");
        pane.raw_output = b"alpha\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("copy-key-table"), Size::new(40, 8))
            .expect("register copy key table client");
        state
            .enter_copy_mode(Some("copy-key-table"), None, false, false, false, false)
            .expect("enter copy key table mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move copy cursor to top");
        state
            .execute_copy_action(pane_id, CopyAction::StartOfLine, 1)
            .expect("move copy cursor to line start");

        state.handle_client_input(client_id, b"\x01", &shared);

        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor_x),
            Some(5)
        );
    }

    #[test]
    fn copy_mode_search_prompt_supports_cursor_editing_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-prompt-edit"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create copy prompt edit session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy prompt edit pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("copy-prompt-edit"), Size::new(40, 8))
            .expect("register copy prompt edit client");
        state
            .enter_copy_mode(Some("copy-prompt-edit"), None, false, false, false, false)
            .expect("enter copy prompt edit mode");

        state.handle_client_input(client_id, b"\x13", &shared);
        state.handle_client_input(
            client_id,
            b"abcd\x1b[D\x1b[DXY\x7f\x1b[1~Z\x1b[4~Y",
            &shared,
        );

        let session_id = state.clients[&client_id].session_id;
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render copy prompt");
        assert!(
            rendered
                .windows(b"(search) ZabXcdY".len())
                .any(|window| window == b"(search) ZabXcdY")
        );
        let mut prompt_terminal = Parser::new(8, 40, 0);
        prompt_terminal.process(&rendered);
        assert_eq!(
            prompt_terminal.screen().cursor_position(),
            (7, 16),
            "copy-mode status prompts own the client cursor"
        );
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(search) ZabXcdY".to_owned())
        );
    }

    #[test]
    fn emacs_incremental_search_prompt_prefills_last_search_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-search-prefill"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create copy search prefill session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = b"first\nneedle target\n";
        let pane = state
            .find_pane_mut(pane_id)
            .expect("copy search prefill pane");
        pane.parser.process(output);
        pane.raw_output = output.to_vec();
        let (_, client_id) = state
            .register_client(Some("copy-search-prefill"), Size::new(40, 8))
            .expect("register copy search prefill client");
        state
            .enter_copy_mode(
                Some("copy-search-prefill"),
                None,
                false,
                false,
                false,
                false,
            )
            .expect("enter copy search prefill mode");
        state
            .execute_copy_action(pane_id, CopyAction::SearchForward("needle".to_owned()), 1)
            .expect("record last copy search");

        state.handle_client_input(client_id, b"\x13", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(CopyModeState::prompt_display),
            Some("(search) needle".to_owned())
        );
    }

    #[test]
    fn copy_mode_prompt_supports_quote_yank_and_history_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-prompt-behavior"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create copy prompt behavior session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state
            .find_pane_mut(pane_id)
            .expect("copy prompt behavior pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("copy-prompt-behavior"), Size::new(40, 8))
            .expect("register copy prompt behavior client");
        state
            .enter_copy_mode(
                Some("copy-prompt-behavior"),
                None,
                false,
                false,
                false,
                false,
            )
            .expect("enter copy prompt behavior mode");

        state.handle_client_input(client_id, b"\x13a\x16\x07b", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(search) a^Gb".to_owned())
        );

        state.handle_client_input(client_id, b"\x15one two\x17\x19\x19", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(search) one twotwo".to_owned())
        );

        state.handle_client_input(client_id, b"\r\x13\x1b[A", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(search) one twotwo".to_owned())
        );

        state.handle_client_input(client_id, b"\r\x1b", &shared);
        state
            .enter_copy_mode(
                Some("copy-prompt-behavior"),
                None,
                false,
                false,
                false,
                false,
            )
            .expect("re-enter copy prompt behavior mode");
        state.handle_client_input(client_id, b"\x13\x1b[A", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(search) one twotwo".to_owned())
        );
    }

    #[test]
    fn csi_u_ctrl_prefix_drives_configured_binding_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("extended-keys", "on", false)
            .expect("enable extended keys");
        state.prefix = vec![1];
        state.bindings.insert(
            vec![b'\\'],
            ConfigBinding {
                _repeat: false,
                commands: vec![vec!["split-window".to_owned(), "-h".to_owned()]],
            },
        );
        state
            .create_session(
                &shared,
                Some("csi-u-prefix"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create csi-u prefix session");
        let (_, client_id) = state
            .register_client(Some("csi-u-prefix"), Size::new(40, 8))
            .expect("register csi-u prefix client");

        state.handle_client_input(client_id, b"\x1b[97;5", &shared);
        assert_eq!(state.sessions[0].windows[0].panes.len(), 1);
        state.handle_client_input(client_id, b"u\\", &shared);

        assert_eq!(state.sessions[0].windows[0].panes.len(), 2);
    }

    #[test]
    fn csi_u_decoding_preserves_meta_and_ctrl_key_contracts() {
        assert_eq!(decode_csi_u(b"127;5"), Some(vec![0x08]));
        assert_eq!(decode_csi_u(b"x;3"), None);
        assert_eq!(decode_csi_u("é".as_bytes()), None);
        assert_eq!(decode_csi_u(b"233;3"), Some(vec![0x1b, 0xc3, 0xa9]));
        assert_eq!(
            decode_extended_key_input(b"\x1b[27;5;97~"),
            (vec![0x01], Vec::new())
        );
    }

    #[test]
    fn attached_emacs_copy_mode_keys_drive_the_copy_state_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("extended-keys", "on", false)
            .expect("enable extended keys");
        state
            .set_global_option("extended-keys-format", "csi-u", false)
            .expect("select CSI-u extended key format");
        state
            .create_session(
                &shared,
                Some("attached-copy-keys"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create attached copy key session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state
            .find_pane_mut(pane_id)
            .expect("attached copy key pane");
        pane.parser.process(b"abcdef\n");
        pane.raw_output = b"abcdef\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("attached-copy-keys"), Size::new(30, 6))
            .expect("register attached copy key client");
        state
            .enter_copy_mode(Some("attached-copy-keys"), None, false, false, false, false)
            .expect("enter attached copy key mode");
        state.prefix = vec![1];
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move attached copy key cursor to history top");
        state
            .execute_copy_action(pane_id, CopyAction::EndOfLine, 1)
            .expect("move attached copy key cursor to line end");
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor_x),
            Some(6)
        );

        state.handle_client_input(client_id, b"\x1b[97;5u", &shared);

        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor_x),
            Some(0)
        );

        state.handle_client_input(client_id, b"g", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .and_then(|mode| mode.prompt_display()),
            Some("(goto line) ".to_owned())
        );
        state.handle_client_input(client_id, b"\x1b", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.hide_position),
            Some(false)
        );
        state.handle_client_input(client_id, b"P", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.hide_position),
            Some(true)
        );
    }

    #[test]
    fn attached_emacs_copy_mode_modified_keys_match_the_builtin_table_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("attached-copy-modifiers"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create modified copy key session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state
            .find_pane_mut(pane_id)
            .expect("modified copy key pane");
        pane.parser.process(b"one\ntwo\nthree\nfour\n");
        pane.raw_output = b"one\ntwo\nthree\nfour\n".to_vec();
        state
            .enter_copy_mode(
                Some("attached-copy-modifiers"),
                None,
                false,
                false,
                false,
                false,
            )
            .expect("enter modified copy key mode");

        let (actions, consumed) = copy_input_actions(&mut state, pane_id, b"\x1b[1;3B");
        assert_eq!(consumed, 6);
        assert_eq!(actions, vec![(CopyAction::HalfPageDown, 1)]);

        let (actions, consumed) = copy_input_actions(&mut state, pane_id, b"\x1b[1;7A");
        assert_eq!(consumed, 6);
        assert_eq!(actions, vec![(CopyAction::PreviousPrompt, 1)]);

        let (actions, consumed) = copy_input_actions(&mut state, pane_id, b"\x1b5\x0e");
        assert_eq!(consumed, 3);
        assert_eq!(actions, vec![(CopyAction::CursorDown, 5)]);
    }

    #[test]
    fn attached_vi_copy_mode_uses_rectangle_toggle_and_space_selection_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mode-keys", "vi", false)
            .expect("enable vi copy keys");
        state
            .create_session(
                &shared,
                Some("attached-vi-selection"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create attached vi selection session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state
            .find_pane_mut(pane_id)
            .expect("attached vi selection pane");
        pane.parser.process(b"abcdef\n");
        pane.raw_output = b"abcdef\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("attached-vi-selection"), Size::new(30, 6))
            .expect("register attached vi selection client");
        state
            .enter_copy_mode(
                Some("attached-vi-selection"),
                None,
                false,
                false,
                false,
                false,
            )
            .expect("enter attached vi selection mode");

        state.handle_client_input(client_id, b"v", &shared);
        let mode = state
            .find_pane(pane_id)
            .and_then(|pane| pane.copy_mode.as_ref())
            .expect("vi copy mode after rectangle toggle");
        assert!(mode.rectangle_selection());
        assert!(!mode.selection_is_active());

        state.handle_client_input(client_id, b" ", &shared);
        let mode = state
            .find_pane(pane_id)
            .and_then(|pane| pane.copy_mode.as_ref())
            .expect("vi copy mode after selection start");
        assert!(mode.rectangle_selection());
        assert!(mode.selection_is_active());
    }

    #[test]
    fn attached_vi_copy_mode_leaves_uppercase_r_unbound_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mode-keys", "vi", false)
            .expect("enable vi copy keys");
        state
            .create_session(
                &shared,
                Some("attached-vi-r"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create attached vi r session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("attached vi r pane");
        pane.parser.process(b"abcdef\n");
        pane.raw_output = b"abcdef\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("attached-vi-r"), Size::new(30, 6))
            .expect("register attached vi r client");
        state
            .enter_copy_mode(Some("attached-vi-r"), None, false, false, false, false)
            .expect("enter attached vi r mode");

        let (actions, consumed) = copy_input_actions(&mut state, pane_id, b"r");
        assert_eq!(consumed, 1);
        assert_eq!(actions, vec![(CopyAction::RefreshFromPane, 1)]);

        state.handle_client_input(client_id, b"R", &shared);

        assert!(
            !state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.rectangle_selection())
        );
    }

    #[test]
    fn copy_line_actions_restore_cursor_and_clear_selection_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-line-state"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create copy-line state session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy-line state pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        state
            .enter_copy_mode(Some("copy-line-state"), None, false, false, false, false)
            .expect("enter copy-line state mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move copy-line state to history top");
        state
            .execute_copy_action(pane_id, CopyAction::StartOfLine, 1)
            .expect("move copy-line state to line start");
        state
            .execute_copy_action(pane_id, CopyAction::CursorRight, 2)
            .expect("move copy-line state cursor");

        state
            .execute_copy_action(pane_id, CopyAction::CopyEndOfLine, 1)
            .expect("copy end of line");
        let mode = state
            .find_pane(pane_id)
            .and_then(|pane| pane.copy_mode.as_ref())
            .expect("copy-line mode after copy");
        assert_eq!(mode.cursor.col, 2);
        assert!(!mode.selection_present());
        assert!(!mode.selection_is_active());
        assert_eq!(
            state.show_buffer(None).expect("copy-line buffer"),
            "pha beta"
        );
    }

    #[test]
    fn repeatable_prefix_bindings_repeat_without_a_second_prefix_headlessly() {
        let config = r###"set -g prefix C-a
bind -r Right next-window
"###;
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state.apply_test_config(config);
        state
            .create_session(
                &shared,
                Some("repeat"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create repeat session");
        state
            .new_window(
                &shared,
                Some("repeat:"),
                Some("second"),
                true,
                None,
                false,
                None,
                false,
                false,
                false,
                true,
                &[],
                None,
            )
            .expect("create repeat window");
        let (_, client_id) = state
            .register_client(Some("repeat"), Size::new(40, 8))
            .expect("register repeat client");

        state.handle_client_input(client_id, b"\x01\x1b[C", &shared);
        assert_eq!(state.sessions[0].active_window, 1);
        state.handle_client_input(client_id, b"\x1b[C", &shared);
        assert_eq!(state.sessions[0].active_window, 0);
    }

    #[test]
    fn sgr_mouse_events_scroll_and_select_in_copy_mode_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .set_global_option("focus-events", "on", false)
            .expect("enable focus reporting");
        state
            .set_global_option("extended-keys", "on", false)
            .expect("enable extended keys");
        state
            .create_session(
                &shared,
                Some("mouse"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 5),
            )
            .expect("create mouse session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("mouse pane");
        pane.parser.process(b"zero\none\ntwo\nthree\nfour\nfive\n");
        pane.raw_output = b"zero\none\ntwo\nthree\nfour\nfive\n".to_vec();
        state
            .enter_copy_mode(Some("mouse"), None, false, false, false, false)
            .expect("enter mouse copy mode");
        let (session_id, client_id) = state
            .register_client(Some("mouse"), Size::new(20, 5))
            .expect("register mouse client");
        let rendered = state
            .render_session(session_id, Some(client_id))
            .expect("render mouse client");
        assert!(
            rendered
                .windows(b"\x1b[?1000h".len())
                .any(|window| window == b"\x1b[?1000h")
        );
        assert!(
            rendered
                .windows(b"\x1b[?1002h".len())
                .any(|window| window == b"\x1b[?1002h")
        );
        assert!(
            rendered
                .windows(b"\x1b[?1006h".len())
                .any(|window| window == b"\x1b[?1006h")
        );
        assert!(
            rendered
                .windows(b"\x1b[?1004h".len())
                .any(|window| window == b"\x1b[?1004h")
        );
        assert!(
            rendered
                .windows(b"\x1b[>1u".len())
                .any(|window| window == b"\x1b[>1u")
        );
        state.last_message = Some("configuration reloaded.".to_owned());
        let message_render = state
            .render_session(session_id, Some(client_id))
            .expect("render display message");
        assert!(
            message_render
                .windows(b"configuration reloaded.".len())
                .any(|window| window == b"configuration reloaded.")
        );

        state.handle_client_input(client_id, b"\x1b[<64;1;1M", &shared);
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.scroll_position() > 0)
        );
        state.handle_client_input(client_id, b"\x1b[<0;1;1M", &shared);
        assert!(
            !state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(CopyModeState::selection_is_active)
        );
        assert!(
            !state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(CopyModeState::selection_present)
        );
        state.handle_client_input(client_id, b"\x1b[<32;6;2M", &shared);
        state.handle_client_input(client_id, b"\x1b[<0;6;2m", &shared);
        let buffer = state.show_buffer(None).expect("mouse selection buffer");
        assert!(!buffer.is_empty());
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_none()
        );
    }

    #[test]
    fn copy_mode_attached_wheel_preserves_stopped_selection_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("wheel-selection"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(40, 10),
            )
            .expect("create wheel selection session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..80)
            .map(|row| format!("line {row:02} xxxxxxxxxx\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("wheel selection pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        state
            .enter_copy_mode(Some("wheel-selection"), None, false, false, false, false)
            .expect("enter wheel selection copy mode");
        for action in [
            (CopyAction::HistoryTop, 1),
            (CopyAction::CursorDown, 10),
            (CopyAction::StartOfLine, 1),
            (CopyAction::BeginSelection, 1),
            (CopyAction::CursorDown, 2),
            (CopyAction::CopySelectionNoClear, 1),
            (CopyAction::StopSelection, 1),
        ] {
            state
                .execute_copy_action(pane_id, action.0, action.1)
                .expect("execute wheel selection action");
        }
        let initial = state.show_buffer(None).expect("read initial selection");
        let (_, client_id) = state
            .register_client(Some("wheel-selection"), Size::new(40, 10))
            .expect("register wheel selection client");

        state.handle_client_input(client_id, b"\x1b[<65;5;5M", &shared);
        state
            .execute_copy_action(pane_id, CopyAction::CopySelectionNoClear, 1)
            .expect("copy after wheel down");
        assert_eq!(
            state.show_buffer(None).expect("read after wheel down"),
            initial
        );

        state.handle_client_input(client_id, b"\x1b[<64;5;5M", &shared);
        state
            .execute_copy_action(pane_id, CopyAction::CopySelectionNoClear, 1)
            .expect("copy after wheel up");
        assert_eq!(
            state.show_buffer(None).expect("read after wheel up"),
            initial
        );
    }

    #[test]
    fn modal_scrollbar_mouse_slider_moves_copy_mode_view_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("scrollbar-mouse"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create scrollbar mouse session");
        state
            .set_window_option(Some("scrollbar-mouse"), "pane-scrollbars", "modal")
            .expect("enable modal scrollbars");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..40)
            .map(|row| format!("line {row:02}\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("scrollbar mouse pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        state
            .enter_copy_mode(Some("scrollbar-mouse"), None, false, true, false, false)
            .expect("enter scrollbar copy mode");
        let (_, client_id) = state
            .register_client(Some("scrollbar-mouse"), Size::new(20, 6))
            .expect("register scrollbar mouse client");

        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(CopyModeState::scroll_position),
            Some(0)
        );
        // Modal scrollbars overlay the rightmost pane cell. Row 3 is inside
        // the slider and should move the copy-mode viewport away from the
        // live edge.
        state.handle_client_input(client_id, b"\x1b[<0;20;3M", &shared);
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.scroll_position() > 0),
            "scrollbar slider did not move copy-mode view"
        );
        let after_press = state
            .find_pane(pane_id)
            .and_then(|pane| pane.copy_mode.as_ref())
            .map(CopyModeState::scroll_position)
            .expect("copy mode after scrollbar press");
        state.handle_client_input(client_id, b"\x1b[<32;20;1M", &shared);
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.scroll_position() > after_press),
            "scrollbar slider drag did not move copy-mode view"
        );
        state.handle_client_input(client_id, b"\x1b[<0;20;1m", &shared);

        state
            .execute_copy_action(pane_id, CopyAction::Cancel, 1)
            .expect("leave modal scrollbar copy mode");
        state
            .set_window_option(Some("scrollbar-mouse"), "pane-scrollbars", "on")
            .expect("enable always-on scrollbars");
        state.handle_client_input(client_id, b"\x1b[<0;20;3M", &shared);
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.scroll_position() > 0),
            "scrollbar click did not enter and page copy mode"
        );
    }

    #[test]
    fn scrollbar_slider_drag_preserves_grabbed_position_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("scrollbar-grab"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create scrollbar grab session");
        state
            .set_window_option(Some("scrollbar-grab"), "pane-scrollbars", "modal")
            .expect("enable modal scrollbar");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..16)
            .map(|row| format!("line {row:02}\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("scrollbar grab pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        state
            .enter_copy_mode(Some("scrollbar-grab"), None, false, true, false, false)
            .expect("enter scrollbar grab copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::PageUp, 1)
            .expect("move scrollbar grab view up");
        state
            .execute_copy_action(pane_id, CopyAction::ScrollDown, 1)
            .expect("position scrollbar grab view");
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(CopyModeState::scroll_position),
            Some(5)
        );
        let (_, client_id) = state
            .register_client(Some("scrollbar-grab"), Size::new(20, 6))
            .expect("register scrollbar grab client");

        // With 10 retained history rows and a six-row viewport, the thumb is
        // two rows tall and starts at row 2 while the view is at offset 5.
        // Grab its second row (terminal row 4), then report motion at the
        // same row. The view must remain at offset 5; treating the pointer
        // as the thumb's top would move it.
        state.handle_client_input(client_id, b"\x1b[<0;20;4M", &shared);
        state.handle_client_input(client_id, b"\x1b[<32;20;4M", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(CopyModeState::scroll_position),
            Some(5)
        );
        state.handle_client_input(client_id, b"\x1b[<0;20;4m", &shared);
    }

    #[test]
    fn pane_scrollbars_render_styled_track_and_thumb_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("scrollbar-render"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create scrollbar render session");
        state
            .set_window_option(Some("scrollbar-render"), "pane-scrollbars", "on")
            .expect("enable rendered scrollbar");
        state
            .set_window_option(
                Some("scrollbar-render"),
                "pane-scrollbars-style",
                "bg=black,fg=white,width=1,pad=0",
            )
            .expect("set rendered scrollbar style");
        state
            .set_global_option("status", "off", false)
            .expect("hide status for scrollbar render");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..40)
            .map(|row| format!("line {row:02}\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("scrollbar render pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        let session_id = state.sessions[0].id;
        let rendered = state
            .render_session(session_id, None)
            .expect("render scrollbar session");
        let track = b"\x1b[1;20H\x1b[40m\x1b[37m ";
        let thumb = b"\x1b[6;20H\x1b[47m\x1b[30m ";
        assert!(rendered.windows(track.len()).any(|window| window == track));
        assert!(rendered.windows(thumb.len()).any(|window| window == thumb));
    }

    #[test]
    fn copy_mode_scrollbar_render_tracks_the_scrollback_view_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("scrollbar-copy-render"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create copy scrollbar session");
        state
            .set_window_option(Some("scrollbar-copy-render"), "pane-scrollbars", "on")
            .expect("enable copy scrollbar");
        state
            .set_window_option(
                Some("scrollbar-copy-render"),
                "pane-scrollbars-style",
                "bg=black,fg=white,width=1,pad=0",
            )
            .expect("set copy scrollbar style");
        state
            .set_global_option("status", "off", false)
            .expect("hide status for copy scrollbar");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..40)
            .map(|row| format!("line {row:02}\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("copy scrollbar pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        state
            .enter_copy_mode(
                Some("scrollbar-copy-render"),
                None,
                false,
                true,
                false,
                false,
            )
            .expect("enter copy scrollbar mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move copy scrollbar to history top");
        let session_id = state.sessions[0].id;
        let mut terminal = Parser::new(6, 20, 10_000);
        terminal.process(
            &state
                .render_session(session_id, None)
                .expect("render copy scrollbar at history top"),
        );
        let top_cell = terminal.screen().cell(0, 19).expect("top scrollbar cell");
        let bottom_cell = terminal
            .screen()
            .cell(5, 19)
            .expect("bottom scrollbar cell");
        assert_eq!(top_cell.bgcolor(), vt100::Color::Idx(7));
        assert_eq!(top_cell.fgcolor(), vt100::Color::Idx(0));
        assert_eq!(bottom_cell.bgcolor(), vt100::Color::Idx(0));
        assert_eq!(bottom_cell.fgcolor(), vt100::Color::Idx(7));

        state
            .execute_copy_action(pane_id, CopyAction::HistoryBottom, 1)
            .expect("move copy scrollbar to history bottom");
        let mut terminal = Parser::new(6, 20, 10_000);
        terminal.process(
            &state
                .render_session(session_id, None)
                .expect("render copy scrollbar at history bottom"),
        );
        let top_cell = terminal.screen().cell(0, 19).expect("top scrollbar cell");
        let bottom_cell = terminal
            .screen()
            .cell(5, 19)
            .expect("bottom scrollbar cell");
        assert_eq!(top_cell.bgcolor(), vt100::Color::Idx(0));
        assert_eq!(top_cell.fgcolor(), vt100::Color::Idx(7));
        assert_eq!(bottom_cell.bgcolor(), vt100::Color::Idx(7));
        assert_eq!(bottom_cell.fgcolor(), vt100::Color::Idx(0));
    }

    #[test]
    fn pane_scrollbars_reserve_left_track_and_pad_for_pane_content_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("scrollbar-left-render"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create left scrollbar session");
        state
            .set_window_option(Some("scrollbar-left-render"), "pane-scrollbars", "on")
            .expect("enable left scrollbar");
        state
            .set_window_option(
                Some("scrollbar-left-render"),
                "pane-scrollbars-position",
                "left",
            )
            .expect("place scrollbar on the left");
        state
            .set_window_option(
                Some("scrollbar-left-render"),
                "pane-scrollbars-style",
                "bg=black,fg=white,width=2,pad=1",
            )
            .expect("set left scrollbar style");
        state
            .set_global_option("status", "off", false)
            .expect("hide status for left scrollbar render");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..40)
            .map(|row| format!("SB{row:02} abcdefghij\r\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("left scrollbar pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();
        let session_id = state.sessions[0].id;
        let rendered = state
            .render_session(session_id, None)
            .expect("render left scrollbar session");
        let mut parser = Parser::new(6, 20, 10_000);
        parser.process(&rendered);
        let first_row = parser
            .screen()
            .contents()
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        assert!(
            first_row.starts_with("   SB35"),
            "unexpected left row: {first_row:?}"
        );
    }

    #[test]
    fn copy_mode_mouse_flags_use_event_context_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("copy-mouse-flags"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 6),
            )
            .expect("create copy mouse flags session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = (0..40)
            .map(|row| format!("line {row:02}\n"))
            .collect::<String>();
        let pane = state.find_pane_mut(pane_id).expect("copy mouse flags pane");
        pane.parser.process(output.as_bytes());
        pane.raw_output = output.into_bytes();

        state.mouse_context = Some(MouseContext {
            x: 3,
            y: 2,
            pane_id,
            word: String::new(),
            line: String::new(),
            hyperlink: String::new(),
            button: 1,
        });
        execute_request(
            &mut state,
            &shared,
            Request::CopyMode {
                target: None,
                source: None,
                exit_on_scroll: false,
                hide_position: true,
                kill_on_exit: false,
                page: false,
                page_down: false,
                reset: false,
                mouse_start: true,
                scroll_to_mouse: false,
            },
        )
        .expect("start copy mode at mouse");
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(CopyModeState::selection_is_active)
        );

        state
            .execute_copy_action(pane_id, CopyAction::Cancel, 1)
            .expect("leave mouse-start copy mode");
        state.mouse_context.as_mut().expect("mouse context").y = 1;
        execute_request(
            &mut state,
            &shared,
            Request::CopyMode {
                target: None,
                source: None,
                exit_on_scroll: false,
                hide_position: true,
                kill_on_exit: false,
                page: false,
                page_down: false,
                reset: false,
                mouse_start: false,
                scroll_to_mouse: true,
            },
        )
        .expect("scroll copy mode at mouse");
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some_and(|mode| mode.scroll_position() > 0)
        );
        state.mouse_context = None;
    }

    #[test]
    fn copy_mode_incremental_search_commands_keep_their_action_kind() {
        assert!(matches!(
            parse_copy_action("search-forward-incremental\0needle"),
            Ok(CopyAction::SearchForwardIncremental(search)) if search == "needle"
        ));
        assert!(matches!(
            parse_copy_action("search-backward-incremental\0needle"),
            Ok(CopyAction::SearchBackwardIncremental(search)) if search == "needle"
        ));
        assert!(matches!(
            parse_copy_action("search-forward-incremental\0=needle"),
            Ok(CopyAction::SearchForwardIncremental(search)) if search == "needle"
        ));
        assert!(matches!(
            parse_copy_action("search-forward-incremental\0-needle"),
            Ok(CopyAction::SearchBackwardIncremental(search)) if search == "needle"
        ));
        assert!(matches!(
            parse_copy_action("search-backward-incremental\0=needle"),
            Ok(CopyAction::SearchBackwardIncremental(search)) if search == "needle"
        ));
    }

    #[test]
    fn copy_pipe_commands_accept_tmux_optional_command_arguments() {
        assert!(matches!(
            parse_copy_action("copy-pipe-and-cancel"),
            Ok(CopyAction::CopyPipe {
                command,
                clear: true,
                cancel: true,
                store: true,
            }) if command.is_empty()
        ));
        assert!(matches!(
            parse_copy_action("copy-pipe-end-of-line-and-cancel"),
            Ok(CopyAction::CopyPipeEndOfLine { command, cancel: true })
                if command.is_empty()
        ));
    }

    #[test]
    fn copy_selection_commands_preserve_tmux_buffer_prefixes() {
        assert!(matches!(
            parse_copy_action("copy-selection-and-cancel\0named-"),
            Ok(CopyAction::CopySelectionWithOptions {
                prefix: Some(prefix),
                clear: true,
                cancel: true,
                set_paste: true,
                set_clipboard: true,
            }) if prefix == "named-"
        ));
        assert!(matches!(
            parse_copy_action("copy-line\0line-"),
            Ok(CopyAction::CopyLineWithOptions {
                prefix: Some(prefix),
                whole_line: true,
                cancel: false,
                set_paste: true,
                set_clipboard: true,
            }) if prefix == "line-"
        ));
    }

    #[test]
    fn copy_pipe_keys_use_global_copy_command_headlessly() {
        let path = std::env::temp_dir().join(format!("tm-copy-command-{}.txt", std::process::id()));
        let _ = fs::remove_file(&path);
        let command = format!("cat > {}", path.display());
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("copy-command", &command, false)
            .expect("set copy command");
        state
            .create_session(
                &shared,
                Some("copy-command"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create copy command session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("copy command pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("copy-command"), Size::new(30, 6))
            .expect("register copy command client");
        state
            .enter_copy_mode(Some("copy-command"), None, false, false, false, false)
            .expect("enter copy command mode");
        for action in [
            CopyAction::HistoryTop,
            CopyAction::StartOfLine,
            CopyAction::BeginSelection,
            CopyAction::EndOfLine,
        ] {
            state
                .execute_copy_action(pane_id, action, 1)
                .expect("prepare copy command selection");
        }

        state.handle_client_input(client_id, b"\x17", &shared);

        assert_eq!(
            fs::read_to_string(&path).expect("copy command output"),
            "alpha beta"
        );
        assert_eq!(
            state.show_buffer(None).expect("copy command buffer"),
            "alpha beta"
        );
        assert!(
            state
                .find_pane(pane_id)
                .is_some_and(|pane| pane.copy_mode.is_none())
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn copy_mode_incremental_search_reuses_its_origin_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("incremental-origin"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 5),
            )
            .expect("create incremental search session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let output = b"xneedle early\nmiddle\nneedle late\n";
        let pane = state
            .find_pane_mut(pane_id)
            .expect("incremental search pane");
        pane.parser.process(output);
        pane.raw_output = output.to_vec();
        state
            .enter_copy_mode(Some("incremental-origin"), None, false, true, false, false)
            .expect("enter incremental copy mode");
        state
            .execute_copy_action(pane_id, CopyAction::HistoryTop, 1)
            .expect("move incremental search to history top");
        state
            .execute_copy_action(pane_id, CopyAction::StartOfLine, 1)
            .expect("move incremental search to line start");
        state
            .execute_copy_action(
                pane_id,
                CopyAction::SearchForwardIncremental("middle".to_owned()),
                1,
            )
            .expect("first incremental search");
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor.row),
            Some(1)
        );
        state
            .execute_copy_action(
                pane_id,
                CopyAction::SearchForwardIncremental("needle".to_owned()),
                1,
            )
            .expect("second incremental search");
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor.row),
            Some(0)
        );
    }

    #[test]
    fn default_mouse_double_and_triple_click_copy_word_and_line_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-clicks"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create mouse click session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("mouse click pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("mouse-clicks"), Size::new(30, 6))
            .expect("register mouse click client");

        let click = b"\x1b[<0;3;1M";
        state.handle_client_input(client_id, click, &shared);
        state.handle_client_input(client_id, click, &shared);
        assert_eq!(
            state.show_buffer(None).expect("double-click buffer"),
            "alpha"
        );
        state.handle_client_input(client_id, click, &shared);
        assert_eq!(
            state.show_buffer(None).expect("triple-click buffer"),
            "alpha beta"
        );
    }

    #[test]
    fn default_mouse_drag_enters_copy_mode_and_copies_on_release_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-drag"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create mouse drag session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("mouse drag pane");
        pane.parser.process(b"alpha beta\n");
        pane.raw_output = b"alpha beta\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("mouse-drag"), Size::new(30, 6))
            .expect("register mouse drag client");

        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);
        state.handle_client_input(client_id, b"\x1b[<32;3;1M", &shared);
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_some()
        );
        state.handle_client_input(client_id, b"\x1b[<32;8;1M", &shared);
        state.handle_client_input(client_id, b"\x1b[<0;8;1m", &shared);

        assert_eq!(state.show_buffer(None).expect("drag buffer"), "pha b");
        assert!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .is_none()
        );
    }

    #[test]
    fn default_mouse_middle_click_pastes_the_active_buffer_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let command = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "stty raw -echo; exec cat -v".to_owned(),
        ];
        let client_id;
        {
            let mut state = shared.lock().expect("server state lock");
            state
                .set_global_option("mouse", "on", false)
                .expect("enable mouse");
            state
                .create_session(
                    &shared,
                    Some("mouse-paste"),
                    false,
                    None,
                    None,
                    None,
                    false,
                    &command,
                    None,
                    Size::new(30, 6),
                )
                .expect("create mouse paste session");
            state.store_buffer(None, b"middle paste".to_vec(), false);
            (_, client_id) = state
                .register_client(Some("mouse-paste"), Size::new(30, 6))
                .expect("register mouse paste client");
            state.handle_client_input(client_id, b"\x1b[<1;1;1M", &shared);
        }

        thread::sleep(Duration::from_millis(100));
        let mut state = shared.lock().expect("server state lock");
        let captured = state
            .capture_pane(Some("mouse-paste:0.0"), None, None, false, false, false)
            .expect("capture mouse paste pane");
        assert!(
            captured.contains("^[[200~middle paste^[[201~"),
            "capture: {captured:?}"
        );
    }

    #[test]
    fn copy_mode_mouse_columns_map_wide_cells_to_one_character_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-wide"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 5),
            )
            .expect("create wide mouse session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("wide mouse pane");
        pane.parser.process("abc中\n".as_bytes());
        pane.raw_output = "abc中\n".as_bytes().to_vec();
        state
            .enter_copy_mode(Some("mouse-wide"), None, false, false, false, false)
            .expect("enter wide mouse copy mode");
        let (_, client_id) = state
            .register_client(Some("mouse-wide"), Size::new(20, 5))
            .expect("register wide mouse client");

        // SGR column 5 is terminal cell column 4 (0-based), the continuation
        // cell of the wide character at character column 3.
        state.handle_client_input(client_id, b"\x1b[<0;5;1M\x1b[<0;5;1m", &shared);
        assert_eq!(
            state
                .find_pane(pane_id)
                .and_then(|pane| pane.copy_mode.as_ref())
                .map(|mode| mode.cursor_x),
            Some(3)
        );
    }

    #[test]
    fn mouse_click_selects_a_pane_when_mouse_mode_is_enabled_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-select"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create mouse selection session");
        state
            .split_window(
                &shared,
                Some("mouse-select:"),
                true,
                false,
                false,
                false,
                false,
                true,
                None,
                &[],
                None,
            )
            .expect("split mouse selection session");
        let window = &mut state.sessions[0].windows[0];
        let first_pane = window.panes[0].id;
        let second_pane = window.panes[1].id;
        window.active_pane = first_pane;
        let second_rect = window.pane(second_pane).expect("second pane").rect;
        let (_, client_id) = state
            .register_client(Some("mouse-select"), Size::new(30, 6))
            .expect("register mouse selection client");

        let click = format!(
            "\x1b[<0;{};{}M",
            second_rect.x.saturating_add(1),
            second_rect.y.saturating_add(1)
        );
        state.handle_client_input(client_id, click.as_bytes(), &shared);
        assert_eq!(
            state.sessions[0].windows[0].active_pane, second_pane,
            "mouse click did not select the clicked pane"
        );
        let first_rect = state.sessions[0].windows[0]
            .pane(first_pane)
            .expect("first pane")
            .rect;
        let wheel = format!(
            "\x1b[<64;{};{}M",
            first_rect.x.saturating_add(1),
            first_rect.y.saturating_add(1)
        );
        state.handle_client_input(client_id, wheel.as_bytes(), &shared);
        assert!(
            state
                .find_pane(first_pane)
                .is_some_and(|pane| pane.copy_mode.is_some())
        );
    }

    #[test]
    fn pane_mouse_bindings_expand_mouse_format_context_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-format"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create mouse format session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("mouse format pane");
        pane.parser.process(b"alpha beta gamma\n");
        pane.raw_output = b"alpha beta gamma\n".to_vec();
        let (_, client_id) = state
            .register_client(Some("mouse-format"), Size::new(30, 6))
            .expect("register mouse format client");
        let line = config::parse(
            r###"bind -n MouseDown1Pane set -g @mouse "x=#{mouse_x} y=#{mouse_y} word=#{mouse_word} line=#{mouse_line} pane=#{mouse_pane}"###,
        )
        .into_iter()
        .next()
        .expect("parse mouse binding");
        assert_eq!(
            line.tokens.last().map(String::as_str),
            Some(
                "x=#{mouse_x} y=#{mouse_y} word=#{mouse_word} line=#{mouse_line} pane=#{mouse_pane}"
            )
        );
        state
            .execute_config_line(client_id, line, &shared)
            .expect("install mouse binding");
        state.mouse_context = Some(MouseContext {
            x: 2,
            y: 0,
            pane_id,
            word: "alpha".to_owned(),
            line: "alpha beta gamma".to_owned(),
            hyperlink: String::new(),
            button: 1,
        });
        assert_eq!(
            state.expand_mouse_format(
                "x=#{mouse_x} y=#{mouse_y} word=#{mouse_word} line=#{mouse_line} pane=#{mouse_pane}"
            ),
            "x=2 y=0 word=alpha line=alpha beta gamma pane=%0"
        );
        state.mouse_context = None;
        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);
        assert_eq!(
            state.global_options.get("@mouse"),
            Some(&"x=2 y=0 word=alpha line=alpha beta gamma pane=%0".to_owned())
        );

        for line in config::parse(r###"bind -n DoubleClick1Pane set -g @double "#{mouse_word}""###)
            .into_iter()
            .chain(config::parse(
                r###"bind -n TripleClick1Pane set -g @triple "#{mouse_word}""###,
            ))
        {
            state
                .execute_config_line(client_id, line, &shared)
                .expect("install click-count binding");
        }
        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);
        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);
        assert_eq!(
            state.global_options.get("@double"),
            Some(&"alpha".to_owned())
        );
        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);
        assert_eq!(
            state.global_options.get("@triple"),
            Some(&"alpha".to_owned())
        );
    }

    #[test]
    fn pane_mouse_bindings_report_osc8_hyperlinks_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .set_global_option("mouse", "on", false)
            .expect("enable mouse");
        state
            .create_session(
                &shared,
                Some("mouse-hyperlink"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(30, 6),
            )
            .expect("create mouse hyperlink session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("mouse hyperlink pane");
        pane.raw_output = b"\x1b]8;;https://example.com\x1b\\LINKED\x1b]8;;\x1b\\\n".to_vec();
        pane.parser.process(&pane.raw_output);
        let (_, client_id) = state
            .register_client(Some("mouse-hyperlink"), Size::new(30, 6))
            .expect("register mouse hyperlink client");
        let line =
            config::parse(r###"bind -n MouseDown1Pane set -g @hyperlink "#{mouse_hyperlink}"###)
                .into_iter()
                .next()
                .expect("parse hyperlink mouse binding");
        state
            .execute_config_line(client_id, line, &shared)
            .expect("install hyperlink mouse binding");

        state.handle_client_input(client_id, b"\x1b[<0;3;1M", &shared);

        assert_eq!(
            state.global_options.get("@hyperlink"),
            Some(&"https://example.com".to_owned())
        );
    }

    #[test]
    fn swap_window_without_a_source_swaps_the_active_window_headlessly() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("swap-config"),
                false,
                None,
                None,
                Some("first"),
                true,
                &[],
                None,
                Size::new(40, 8),
            )
            .expect("create swap session");
        state
            .new_window(
                &shared,
                Some("swap-config:"),
                Some("second"),
                true,
                None,
                false,
                None,
                false,
                false,
                false,
                true,
                &[],
                None,
            )
            .expect("create second window");
        state
            .swap_window(None, Some("swap-config:-1"), false)
            .expect("swap active and relative target");
        assert_eq!(
            state
                .list_windows(Some("swap-config"), Some("#{window_index}:#{window_name}"))
                .expect("list swapped windows"),
            "0:second\n1:first"
        );
    }

    #[test]
    fn raw_output_checkpoint_replays_the_live_terminal_state() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("checkpoint"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 4),
            )
            .expect("create checkpoint session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("checkpoint pane");
        pane.parser
            .process(b"\x1b[31mred\x1b[0m\r\nsecond\x1b[2;4Htail");
        let expected_contents = pane.parser.screen().contents();
        let expected_cursor = pane.parser.screen().cursor_position();
        checkpoint_raw_output(pane);

        assert!(!pane.raw_output.is_empty());
        let mut replayed = Parser::new(4, 20, 100);
        terminal::replay(&mut replayed, &pane.raw_output);
        assert_eq!(replayed.screen().contents(), expected_contents);
        assert_eq!(replayed.screen().cursor_position(), expected_cursor);

        pane.history_floor = 7;
        pane.parser.process(b"\x1b[2J\x1b[Hcheckpoint");
        retain_raw_output(pane, &vec![b'x'; RAW_OUTPUT_LIMIT + 1]);
        assert!(pane.raw_output.len() < RAW_OUTPUT_LIMIT);
        assert_eq!(pane.history_floor, 7);
        let mut replayed = Parser::new(4, 20, 100);
        terminal::replay(&mut replayed, &pane.raw_output);
        assert_eq!(
            replayed.screen().contents(),
            pane.parser.screen().contents()
        );
    }

    #[test]
    fn clear_history_keeps_live_and_replayed_terminal_state_aligned() {
        let shared = Arc::new(Mutex::new(ServerState::new()));
        let mut state = shared.lock().expect("server state lock");
        state
            .create_session(
                &shared,
                Some("clear-history"),
                false,
                None,
                None,
                None,
                true,
                &[],
                None,
                Size::new(20, 4),
            )
            .expect("create clear-history session");
        let pane_id = state.sessions[0].windows[0].panes[0].id;
        let pane = state.find_pane_mut(pane_id).expect("clear-history pane");
        let output = b"old-0\r\nold-1\r\nold-2\r\nold-3\r\nvisible\x1b[2;4Htail";
        pane.parser.process(output);
        pane.raw_output = output.to_vec();

        execute_request(
            &mut state,
            &shared,
            Request::ClearHistory {
                target: Some("clear-history:0.0".to_owned()),
            },
        )
        .expect("clear history");

        let pane = state.find_pane(pane_id).expect("cleared pane");
        let mut replayed = Parser::new(4, 20, 100);
        terminal::replay(&mut replayed, &pane.raw_output);
        assert_eq!(
            replayed.screen().contents(),
            pane.parser.screen().contents()
        );
        assert_eq!(
            replayed.screen().cursor_position(),
            pane.parser.screen().cursor_position()
        );
    }
}
