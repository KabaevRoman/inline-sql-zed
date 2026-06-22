# Contributing

Issues and pull requests are welcome at <https://github.com/KabaevRoman/inline-sql-zed>.

## Development

Requirements:

- Rust installed with `rustup`
- Zed
- Optional: Sqruff or another SQL formatter

Run the checks with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --manifest-path server/Cargo.toml
cargo clippy --manifest-path server/Cargo.toml --all-targets -- -D warnings
cargo build --release --target wasm32-wasip1
```

Install the server during local development with:

```sh
cargo install --path server --force
```

In Zed, run `zed: install dev extension` and select this repository.

