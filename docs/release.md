# Release

The pinned toolchain is in [`rust-toolchain.toml`](../rust-toolchain.toml).
The manually triggered GitHub Actions release workflow builds these targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`

Linux executables must have no ELF interpreter or shared-library dependency.
When run through `workflow_dispatch`, the workflow zips the two Linux
executables and creates a prerelease named `release-<sha>`, where `<sha>` is
the full 40-character commit SHA. The macOS executable
is built and tested in the workflow and is not attached to that release. Linux
builds use the toolchain in [`Dockerfile`](../Dockerfile).

For local builds, `make release TARGET=<target>` builds the release. Run
`make verify-release TARGET=<target>` to check Linux static linking. `make install` builds the release, copies it to `~/.local/bin/tm`, and on
macOS applies an ad hoc code signature.

`make qualify` runs `make release` and `make verify-release`, then the full
test suite and `make parity` against the dist binary through `TM_BIN`. The release workflow does
not run it: it runs the tests only in the macOS job and only `make release`
and `make verify-release` for Linux. Run `make qualify` by hand before you
dispatch a release. See [testing](testing.md).

## Cargo registry mirror

[`.cargo/config.toml`](../.cargo/config.toml) replaces crates.io with the
mirror `sparse+https://crates.17166969.xyz/current/crates-io/`. Every build
that fetches crates needs that mirror reachable, and fetches fail when it is
not. `Cargo.lock` checksums still pin the crates.io content.

## Local Linux verification

Use the container platform that matches the host CPU. On an arm64 Mac that is
`linux/arm64` with the `aarch64-unknown-linux-musl` target. Do not use
`linux/amd64` on an arm64 Mac for routine checks: it runs under emulation and
is much slower.

| Host | Docker platform | Rust target |
| --- | --- | --- |
| arm64 macOS | `linux/arm64` | `aarch64-unknown-linux-musl` |
| x86_64 Linux | `linux/amd64` | `x86_64-unknown-linux-musl` |

Build the release image for the host architecture, then run the release build
and static checks inside it:

```sh
docker build --platform linux/arm64 -t tm-builder .
docker run --rm --init --platform linux/arm64 -v "$PWD:/tm" -w /tm \
  tm-builder sh -ec 'make release TARGET=aarch64-unknown-linux-musl &&
    make verify-release TARGET=aarch64-unknown-linux-musl'
```

The image also builds host PTY tests. On an arm64 Mac, use the native target:

```sh
docker run --rm --init --platform linux/arm64 -v "$PWD:/tm" -w /tm \
  tm-builder sh -ec 'apk add --no-cache vim less &&
    MUSL_TARGET=aarch64-unknown-linux-musl MUSL_BUILD_MODE=test musl-cargo test'
```

Use the matching `linux/amd64` and `x86_64-unknown-linux-musl` pair only when
verifying that target, for example on an x86_64 host. Install `vim` and
`less` in the container for the real-app suite. Reference-dependent suites
require the tmux 3.8 build dependencies and `make parity`; the release image
intentionally does not include them.
