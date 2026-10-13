# Release

The pinned toolchain is in [`rust-toolchain.toml`](../rust-toolchain.toml).
The manually triggered GitHub Actions release workflow builds these targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`

Linux produces dynamic and static executables. The workflow packages each
executable as a zip and creates a prerelease named `release-<commit>` when run
through `workflow_dispatch`. Linux builds use the toolchain in
[`Dockerfile`](../Dockerfile).

For local builds, `make release TARGET=<target>` builds the static release and
`make release-dynamic TARGET=<target>` builds the dynamic release. Run
`make verify-release TARGET=<target>` or
`make verify-release-dynamic TARGET=<target>` for the matching checks. On
macOS, `make install` builds the default target, copies it to `~/usr/bin/tm`,
and applies an ad hoc code signature.
