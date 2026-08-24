#![cfg(unix)]
#![allow(dead_code, unused_imports)]

#[path = "../src/client.rs"]
mod client;
#[path = "../src/command.rs"]
mod command;
#[path = "../src/config.rs"]
mod config;
#[path = "../src/copy_mode.rs"]
mod copy_mode;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/protocol.rs"]
mod protocol;
#[path = "../src/pty.rs"]
mod pty;
#[path = "../src/server.rs"]
mod server;
#[path = "../src/terminal.rs"]
mod terminal;

use rustybench::{Bencher, black_box};

const FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(16);
const FRAME_BYTES_BUDGET: usize = 32 * 1024;

fn assert_frame_budget(frame: &[u8], started: std::time::Instant, label: &str) {
    let elapsed = started.elapsed();
    assert!(
        elapsed <= FRAME_BUDGET,
        "{label} exceeded {FRAME_BUDGET:?}: {elapsed:?}"
    );
    assert!(
        frame.len() <= FRAME_BYTES_BUDGET,
        "{label} exceeded {FRAME_BYTES_BUDGET} output bytes: {}",
        frame.len()
    );
}

#[rustybench::bench(
    name = "render_80x24_1_pane",
    sample_count = 20,
    sample_size = 10,
    threads = false,
)]
fn render_80x24_1_pane(bencher: Bencher) {
    let mut fixture = server::RenderBenchmark::new(80, 24, 1);
    bencher.bench_local(|| {
        let started = std::time::Instant::now();
        let frame = fixture.render_frame();
        assert_frame_budget(&frame, started, "80x24 render");
        black_box(frame);
    });
}

#[rustybench::bench(
    name = "render_80x24_2_panes",
    sample_count = 20,
    sample_size = 10,
    threads = false,
)]
fn render_80x24_2_panes(bencher: Bencher) {
    let mut fixture = server::RenderBenchmark::new(80, 24, 2);
    bencher.bench_local(|| {
        let started = std::time::Instant::now();
        let frame = fixture.render_frame();
        assert_frame_budget(&frame, started, "80x24 two-pane render");
        black_box(frame);
    });
}

#[rustybench::bench(
    name = "render_120x40_4_panes",
    sample_count = 20,
    sample_size = 10,
    threads = false,
)]
fn render_120x40_4_panes(bencher: Bencher) {
    let mut fixture = server::RenderBenchmark::new(120, 40, 4);
    bencher.bench_local(|| {
        let started = std::time::Instant::now();
        let frame = fixture.render_frame();
        assert_frame_budget(&frame, started, "120x40 four-pane render");
        black_box(frame);
    });
}

#[rustybench::bench(
    name = "attached_delta_120x40_4_panes",
    sample_count = 20,
    sample_size = 10,
    threads = false,
)]
fn attached_delta_120x40_4_panes(bencher: Bencher) {
    let mut fixture = server::RenderBenchmark::new(120, 40, 4);
    // Prime the client-side terminal model; the measured iterations are the
    // steady-state incremental path used after the initial full frame.
    let started = std::time::Instant::now();
    let initial = fixture.render_delta_frame();
    assert_frame_budget(&initial, started, "120x40 initial attached render");
    bencher.bench_local(|| {
        let started = std::time::Instant::now();
        let frame = fixture.render_delta_frame();
        assert_frame_budget(&frame, started, "120x40 incremental render");
        black_box(frame);
    });
}

fn main() {
    rustybench::main();
}
