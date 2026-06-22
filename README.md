# Inline SQL for Zed
Inline SQL highlights SQL inside host-language strings and formats only those SQL regions. It is split into:

- a small Zed extension that attaches a secondary language server to common languages;
- `inline-sql-lsp`, a Rust server that finds configured regex captures, lexes SQL into native LSP semantic tokens, and invokes a selected formatter.

This architecture is intentional. Zed's Tree-sitter `injections.scm` files are static and belong to the host language, so an independent extension cannot add runtime, settings-driven injection queries to Go, Rust, Python, and other installed grammars.

## Installation

After publication, install **Inline SQL** from Zed's Extensions page. The extension downloads the
matching `inline-sql-lsp` binary from this repository's GitHub Release automatically.

For local development, build and place the server on `PATH`:

```sh
cargo install --path server --force
```

Then run `zed: install dev extension` and select this directory. Install Zed's SQL extension as well
if you want standalone `.sql` language support; inline highlighting does not depend on it.

Automatic server downloads support macOS and Linux on Apple Silicon/AArch64 and x86-64, plus
Windows x86-64. A custom server can still be selected with `lsp.inline-sql.binary.path`.

## Settings

Semantic tokens must be combined with the host language's Tree-sitter highlighting. The following configuration handles the requested multiline Go example and selects `sqruff`:

Sqruff must be installed separately and available on Zed's `PATH`. A complete copyable example is
available in [`examples/zed-settings.jsonc`](examples/zed-settings.jsonc).

```jsonc
{
  "languages": {
    "Go": {
      "semantic_tokens": "combined",
      "formatter": [
        { "language_server": { "name": "gopls" } },
        { "language_server": { "name": "inline-sql" } }
      ]
    }
  },
  "lsp": {
    "inline-sql": {
      "settings": {
        "patterns": {
          "go": [
            "(?s)\\b(?:const|var)\\s+\\w*(?:Query|SQL)\\w*\\s*=\\s*`(?P<sql>.*?)`",
            "(?is)\\b(?:const|var)\\s+\\w+\\s*=\\s*`(?P<sql>\\s*(?:SELECT|INSERT|UPDATE|DELETE|WITH|MERGE|CREATE|ALTER|DROP|TRUNCATE)\\b.*?)`"
          ]
        },
        "dialect": "postgres",
        "formatter": "sqruff",
        "format_indent": "\t",
        "formatters": {
          "sqruff": {
            "command": "sqruff",
            "args": [
              "fix",
              "--format",
              "json",
              "--config",
              "/absolute/path/to/inline-sql-zed/examples/.sqruff",
              "--dialect",
              "{dialect}",
              "-"
            ]
          },
          "custom": {
            "command": "/absolute/path/to/my-sql-formatter",
            "args": ["--stdin"]
          }
        },
        "formatter_by_language": {
          "python": "custom"
        }
      }
    }
  }
}
```

Each regex should expose the SQL body as a named capture called `sql`. Capture group 1 is used as a fallback, followed by the whole match. Rust's `regex` syntax is used; `(?s)` makes `.` match newlines. Multiple expressions per language are supported. Language keys are case-insensitive and may be Zed names (`Go`) or LSP IDs (`go`).

The server includes defaults for Go, Rust, JavaScript/TypeScript, Python, Java, Kotlin, C#, Ruby, PHP, Elixir, Scala, Swift, C, and C++. Defining `patterns` in settings replaces the default mapping, which keeps matching explicit and predictable.

PostgreSQL is the default dialect. `postgresql`, `psql`, and `pg` are accepted aliases for `postgres`. Other built-in dialect profiles are `ansi`, `mysql`, `mariadb`, `sqlite`, `tsql`, `bigquery`, `snowflake`, `redshift`, `duckdb`, `oracle`, and `clickhouse`. `dialect_by_language` overrides the global selection for a host language.

PostgreSQL highlighting includes PostgreSQL-specific keywords and types, placeholders, `::` casts, quoted identifiers, and dollar-quoted strings. `$1`, `$name`, and `:name` placeholders use the visibly styled semantic `property` token. T-SQL `@name` and MySQL/SQLite `?` placeholders are supported as well. Explicit aliases (`AS user_id`), CTE names, implicit table aliases (`FROM users u`), and all references to those aliases (`u.id`) also receive the `property` token so themes can distinguish them from normal identifiers.

Formatter commands receive one captured SQL region on stdin and must return formatted SQL on stdout. Arguments may contain `{language}` and `{dialect}` placeholders. Formatting reports command failures to Zed and leaves the source unchanged.

`format_indent` prefixes every non-empty formatted SQL line after the external formatter returns. Use this for host-language placement, for example `"\t"` to keep multiline Go raw-string SQL one tab in from the backtick line. `format_indent_by_language` can override it per host language. SQL layout inside the query, such as whether `VALUES` or `DO NOTHING` is indented, remains the formatter's responsibility; pass a formatter config with arguments such as sqruff's `--config /absolute/path/to/inline-sql-zed/examples/.sqruff`. A Sqruff example is available in [`examples/.sqruff`](examples/.sqruff).

When formatting fails, Inline SQL publishes a warning diagnostic on the exact SQL line reported by
the formatter, with its exit status and error message. Sqruff failures trigger a diagnostic lint
pass so its JSON line information can be mapped back into the host file. Formatters that do not
report a line fall back to marking the captured SQL region. Editing the document or formatting
successfully clears the warning.

To override the server executable instead of installing it on `PATH`:

```jsonc
{
  "lsp": {
    "inline-sql": {
      "binary": {
        "path": "/absolute/path/to/inline-sql-lsp"
      }
    }
  }
}
```

## Go example

```go
const someNameQuery = `SELECT users.id, users.email
FROM users
WHERE users.active = TRUE
ORDER BY users.id`
```

The default Go rules capture everything between the backticks, including newlines. SQL strings assigned to names containing `Query` or `SQL` are highlighted. Other `const` or `var` raw strings are highlighted when their first statement token is a common SQL keyword such as `SELECT`, `UPDATE`, or `WITH`; ordinary strings are left alone.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --manifest-path server/Cargo.toml
cargo clippy --manifest-path server/Cargo.toml --all-targets -- -D warnings
cargo build --release --target wasm32-wasip1
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for development details and [SUBMISSION.md](SUBMISSION.md)
for the marketplace release checklist.

## License

MIT © [Roman Kabaev](https://github.com/KabaevRoman). See [LICENSE](LICENSE)

## AI Disclosure
This extension is vibecoded, to solve my own problems with lack of extensions for this exact problem.
