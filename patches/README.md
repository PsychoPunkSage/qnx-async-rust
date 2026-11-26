# Patched crates

## `tokio-1.54.1`

tokio 1.54.1 from crates.io, unchanged except that every `target_os = "nto"`
cfg in these three files also accepts `target_os = "qnx"`, the `target_os` of
the QNX 8 targets (`x86_64-pc-qnx`, `aarch64-unknown-qnx`):

- `src/net/unix/ucred.rs`
- `src/fs/read_dir.rs`
- `src/process/mod.rs`

Without it, tokio fails to build for those targets (`get_peer_cred` is not
found). `tests/` and `CHANGELOG.md` were left out to keep the copy small.

`bench/Cargo.toml` points at this copy through `[patch.crates-io]`. Remove the
copy and that entry once a tokio release supports `target_os = "qnx"`.
