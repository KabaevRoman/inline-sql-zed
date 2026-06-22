# Zed Extension Submission Checklist

Use this checklist when publishing Inline SQL as an official Zed extension.

## Before opening the PR

- Confirm `extension.toml`, root `Cargo.toml`, and `server/Cargo.toml` all use the same version.
- Update `CHANGELOG.md` for the release.
- Run the full validation suite:

```sh
cargo fmt --all -- --check
cargo fmt --manifest-path server/Cargo.toml --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path server/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path server/Cargo.toml
cargo build --release --target wasm32-wasip1
```

- Push a `v<version>` tag so `.github/workflows/release.yml` creates the server binary archives expected by `src/lib.rs`.
- Verify the GitHub Release contains these assets:
  - `inline-sql-lsp-aarch64-apple-darwin.tar.gz`
  - `inline-sql-lsp-x86_64-apple-darwin.tar.gz`
  - `inline-sql-lsp-aarch64-unknown-linux-gnu.tar.gz`
  - `inline-sql-lsp-x86_64-unknown-linux-gnu.tar.gz`
  - `inline-sql-lsp-x86_64-pc-windows-msvc.zip`

## Official Zed extension PR

- Fork <https://github.com/zed-industries/extensions>.
- Add this repository as the `inline-sql` extension following the current instructions in that repository.
- Point the entry at the same release tag as `extension.toml`.
- Mention that the extension downloads the matching `inline-sql-lsp` server release assets on first use.
