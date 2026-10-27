# Release

The pinned toolchain is in [`rust-toolchain.toml`](../rust-toolchain.toml).
The manually triggered GitHub Actions release workflow builds these targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`

Linux executables must have no ELF interpreter or shared-library dependency.
The workflow packages each executable as a zip and creates a prerelease named
`release-<commit>` when run through `workflow_dispatch`. Linux builds use the toolchain in
[`Dockerfile`](../Dockerfile).

For local builds, `make release TARGET=<target>` builds the release. Run
`make verify-release TARGET=<target>` to check Linux static linking. On
macOS, `make install` builds the default target, copies it to `~/.local/bin/tm`,
and applies an ad hoc code signature.
