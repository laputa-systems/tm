use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::parity::Mux;
use super::raw_pty::RawClient;

const SENTINEL: u8 = b'Z';
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Sink {
    path: PathBuf,
    consumed: usize,
}

impl Sink {
    pub fn start(mux: &Mux, session: &str, window_name: &str, private_modes: &str) -> Self {
        let (path, script) = sink_script(mux, session, window_name, private_modes);
        mux.ok(&[
            "new-session", "-d", "-s", session, "-n", window_name, "-x", "80", "-y", "24", "--",
            "sh", "-c", &script,
        ]);
        mux.capture_until(session, "READY", TIMEOUT);
        Self { path, consumed: 0 }
    }

    pub fn deliver_paused(
        &mut self,
        client: &mut RawClient,
        head: &[u8],
        pause: Duration,
        tail: &[u8],
    ) -> Vec<u8> {
        client.write(head);
        std::thread::sleep(pause);
        client.write(tail);
        client.write(&[SENTINEL]);
        self.wait_for_sentinel("paused")
    }

    fn contents(&self) -> Vec<u8> {
        std::fs::read(&self.path).unwrap_or_default()
    }

    fn wait_for_sentinel(&mut self, what: &str) -> Vec<u8> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let all = self.contents();
            let fresh = &all[self.consumed..];
            if fresh.last() == Some(&SENTINEL) {
                self.consumed = all.len();
                return fresh[..fresh.len() - 1].to_vec();
            }
            assert!(
                Instant::now() < deadline,
                "{what}: no sentinel; pane received {fresh:?}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn deliver(&mut self, client: &mut RawClient, chunks: &[&[u8]], what: &str) -> Vec<u8> {
        for chunk in chunks {
            client.write(chunk);
        }
        client.write(&[SENTINEL]);
        self.wait_for_sentinel(what)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Observed {
    pub on_attach: Vec<u8>,
    pub unsplit: Vec<u8>,
    pub per_boundary: Vec<Vec<u8>>,
    pub fragmented: Vec<u8>,
}

pub fn observe(sink: &mut Sink, client: &mut RawClient, input: &[u8]) -> Observed {
    let on_attach = sink.deliver(client, &[], "attach");
    let unsplit = sink.deliver(client, &[input], "unsplit");
    let per_boundary = (1..input.len())
        .map(|at| {
            let (head, tail) = input.split_at(at);
            sink.deliver(client, &[head, tail], &format!("split at {at}"))
        })
        .collect();
    let singles: Vec<&[u8]> = input.chunks(1).collect();
    let fragmented = sink.deliver(client, &singles, "fragmented");
    Observed { on_attach, unsplit, per_boundary, fragmented }
}

pub fn assert_split_invariant(label: &str, mux: &Mux, observed: &Observed) {
    for (index, bytes) in observed.per_boundary.iter().enumerate() {
        assert_eq!(
            bytes,
            &observed.unsplit,
            "{label} {:?}: split at byte {} changed the pane bytes",
            mux.implementation(),
            index + 1
        );
    }
    assert_eq!(
        observed.fragmented,
        observed.unsplit,
        "{label} {:?}: byte-by-byte delivery changed the pane bytes",
        mux.implementation()
    );
}

fn sink_script(
    mux: &Mux,
    session: &str,
    window_name: &str,
    private_modes: &str,
) -> (PathBuf, String) {
    let path = mux.root().join(format!("sink-{session}-{window_name}.bin"));
    let script = format!(
        "stty raw -echo; printf '{private_modes}READY'; exec cat >> '{}'",
        path.display()
    );
    (path, script)
}

pub fn attach(mux: &Mux, session: &str) -> RawClient {
    let client = RawClient::spawn(mux, "split", &["attach-session", "-t", session], 24, 80);
    client.wait_for_text(&format!("({session})"), TIMEOUT);
    client
}
