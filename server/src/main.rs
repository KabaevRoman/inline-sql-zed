mod config;
mod sql;

use config::Settings;
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Debug)]
struct Document {
    language_id: String,
    text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Injection {
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct FormatFailure {
    message: String,
    injection: Injection,
    sql_line: Option<usize>,
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut documents = HashMap::<String, Document>::new();
    let mut settings = Settings::default();
    let mut shutdown = false;

    while let Some(message) = read_message(&mut input)? {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                if let Some(value) = params.get("initializationOptions") {
                    settings.update(value.clone());
                }
                respond(
                    &mut output,
                    id,
                    json!({
                        "capabilities": {
                            "textDocumentSync": 1,
                            "semanticTokensProvider": {
                                "legend": {
                                    "tokenTypes": sql::TOKEN_TYPES,
                                    "tokenModifiers": []
                                },
                                "full": true
                            },
                            "documentFormattingProvider": true
                        },
                        "serverInfo": { "name": "inline-sql-lsp", "version": env!("CARGO_PKG_VERSION") }
                    }),
                )?;
            }
            "workspace/didChangeConfiguration" => {
                let value = params.get("settings").cloned().unwrap_or(params);
                settings.update(value);
            }
            "textDocument/didOpen" => {
                if let Some(item) = params.get("textDocument") {
                    if let (Some(uri), Some(text), Some(language_id)) = (
                        item.get("uri").and_then(Value::as_str),
                        item.get("text").and_then(Value::as_str),
                        item.get("languageId").and_then(Value::as_str),
                    ) {
                        documents.insert(
                            uri.to_string(),
                            Document {
                                language_id: language_id.to_string(),
                                text: text.to_string(),
                            },
                        );
                    }
                }
            }
            "textDocument/didChange" => {
                let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
                let text = params
                    .pointer("/contentChanges/0/text")
                    .and_then(Value::as_str);
                if let (Some(uri), Some(text)) = (uri, text) {
                    if let Some(document) = documents.get_mut(uri) {
                        document.text = text.to_string();
                    }
                    publish_diagnostics(&mut output, uri, Vec::new())?;
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) {
                    documents.remove(uri);
                    publish_diagnostics(&mut output, uri, Vec::new())?;
                }
            }
            "textDocument/semanticTokens/full" => {
                let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
                let data = uri
                    .and_then(|uri| documents.get(uri))
                    .map(|document| semantic_tokens(document, &settings))
                    .unwrap_or_default();
                respond(&mut output, id, json!({ "data": data }))?;
            }
            "textDocument/formatting" => {
                let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
                let result = uri
                    .and_then(|uri| documents.get(uri))
                    .map(|document| format_document(document, &settings))
                    .unwrap_or_else(|| Ok(Vec::new()));
                match result {
                    Ok(edits) => {
                        if let Some(uri) = uri {
                            publish_diagnostics(&mut output, uri, Vec::new())?;
                        }
                        respond(&mut output, id, Value::Array(edits))?;
                    }
                    Err(error) => {
                        if let (Some(uri), Some(document)) =
                            (uri, uri.and_then(|uri| documents.get(uri)))
                        {
                            publish_diagnostics(
                                &mut output,
                                uri,
                                vec![formatter_diagnostic(document, &error)],
                            )?;
                        }
                        respond_error(&mut output, id, -32603, &error.message)?;
                    }
                }
            }
            "shutdown" => {
                shutdown = true;
                respond(&mut output, id, Value::Null)?;
            }
            "exit" => break,
            _ if id.is_some() => respond(&mut output, id, Value::Null)?,
            _ => {}
        }

        if shutdown && method == "exit" {
            break;
        }
    }
    Ok(())
}

fn injections(document: &Document, settings: &Settings) -> Vec<Injection> {
    let mut result = Vec::new();
    for pattern in settings.patterns_for(&document.language_id) {
        let regex = match Regex::new(pattern) {
            Ok(regex) => regex,
            Err(error) => {
                eprintln!(
                    "invalid inline SQL regex for {}: {error}",
                    document.language_id
                );
                continue;
            }
        };
        for captures in regex.captures_iter(&document.text) {
            let matched = captures
                .name("sql")
                .or_else(|| captures.get(1))
                .or_else(|| captures.get(0));
            if let Some(matched) = matched {
                if matched.start() < matched.end() {
                    result.push(Injection {
                        start: matched.start(),
                        end: matched.end(),
                    });
                }
            }
        }
    }
    result.sort_by_key(|item| (item.start, item.end));
    result.dedup();
    result
        .into_iter()
        .fold(Vec::new(), |mut non_overlapping, item| {
            if non_overlapping
                .last()
                .is_none_or(|previous: &Injection| item.start >= previous.end)
            {
                non_overlapping.push(item);
            }
            non_overlapping
        })
}

fn semantic_tokens(document: &Document, settings: &Settings) -> Vec<u32> {
    let mut tokens = Vec::new();
    let dialect = settings.dialect_for(&document.language_id);
    for injection in injections(document, settings) {
        for token in sql::tokenize(&document.text[injection.start..injection.end], &dialect) {
            let start = injection.start + token.start;
            let end = injection.start + token.end;
            for (line, column, length) in token_segments(&document.text, start, end) {
                tokens.push((line, column, length, token.kind));
            }
        }
    }
    tokens.sort_unstable();
    tokens.dedup();

    let mut encoded = Vec::with_capacity(tokens.len() * 5);
    let (mut previous_line, mut previous_column) = (0, 0);
    for (line, column, length, kind) in tokens {
        let delta_line = line - previous_line;
        let delta_column = if delta_line == 0 {
            column - previous_column
        } else {
            column
        };
        encoded.extend([delta_line, delta_column, length, kind, 0]);
        previous_line = line;
        previous_column = column;
    }
    encoded
}

fn format_document(document: &Document, settings: &Settings) -> Result<Vec<Value>, FormatFailure> {
    let sql_injections = injections(document, settings);
    let fallback_range = sql_injections.first().copied().unwrap_or(Injection {
        start: 0,
        end: document.text.len(),
    });
    let formatter = settings
        .formatter_for(&document.language_id)
        .map_err(|message| FormatFailure {
            sql_line: formatter_error_line(&message),
            message,
            injection: fallback_range,
        })?;
    let Some(formatter) = formatter else {
        return Ok(Vec::new());
    };
    let dialect = settings.dialect_for(&document.language_id);
    let format_indent = settings.format_indent_for(&document.language_id);
    let mut edits = Vec::new();
    for injection in sql_injections {
        let sql = &document.text[injection.start..injection.end];
        let mut formatted = run_formatter(formatter, &document.language_id, &dialect, sql)
            .map_err(|message| FormatFailure {
                sql_line: formatter_error_line(&message),
                message,
                injection,
            })?;
        formatted = apply_format_indent(&formatted, format_indent);
        if formatted != sql {
            edits.push(json!({
                "range": {
                    "start": position(&document.text, injection.start),
                    "end": position(&document.text, injection.end)
                },
                "newText": formatted
            }));
        }
    }
    Ok(edits)
}

fn formatter_diagnostic(document: &Document, error: &FormatFailure) -> Value {
    let range = error
        .sql_line
        .and_then(|line| sql_line_range(document, error.injection, line))
        .unwrap_or(error.injection);
    json!({
        "range": {
            "start": position(&document.text, range.start),
            "end": position(&document.text, range.end)
        },
        "severity": 2,
        "source": "inline-sql",
        "code": "formatter-failed",
        "message": error.message
    })
}

fn formatter_error_line(message: &str) -> Option<usize> {
    let sqruff = Regex::new(r"(?i)\bL:\s*(\d+)\s*\|\s*P:\s*\d+").ok()?;
    let conventional = Regex::new(r"(?i)\bline\s+(\d+)(?:\s*[,;:]\s*(?:column\s+)?\d+)?").ok()?;
    for regex in [&sqruff, &conventional] {
        if let Some(line) = regex
            .captures(message)
            .and_then(|captures| captures.get(1))
            .and_then(|line| line.as_str().parse::<usize>().ok())
        {
            return Some(line.max(1));
        }
    }
    None
}

fn sql_line_range(
    document: &Document,
    injection: Injection,
    one_based_line: usize,
) -> Option<Injection> {
    let sql = document.text.get(injection.start..injection.end)?;
    let mut relative_start = 0;
    for (index, line) in sql.split_inclusive('\n').enumerate() {
        let relative_end = relative_start + line.trim_end_matches(['\r', '\n']).len();
        if index + 1 == one_based_line {
            return Some(Injection {
                start: injection.start + relative_start,
                end: injection.start + relative_end.max(relative_start + 1).min(sql.len()),
            });
        }
        relative_start += line.len();
    }
    None
}

fn run_formatter(
    formatter: &config::FormatterCommand,
    language_id: &str,
    dialect: &str,
    input: &str,
) -> Result<String, String> {
    let (formatter_input, placeholders) = mask_placeholders(input, dialect);
    let args = formatter
        .args
        .iter()
        .map(|arg| {
            arg.replace("{language}", language_id)
                .replace("{dialect}", dialect)
        })
        .collect::<Vec<_>>();
    let mut child = Command::new(&formatter.command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", formatter.command))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "formatter stdin was unavailable".to_string())?
        .write_all(formatter_input.as_bytes())
        .map_err(|error| format!("failed to write formatter stdin: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("formatter failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut details = formatter_error_details(&stderr, &stdout);
        if formatter_error_line(&details).is_none() && is_sqruff(&formatter.command) {
            if let Some(diagnostic) = sqruff_lint_diagnostic(
                &formatter.command,
                dialect,
                &formatter_input,
                &formatter.args,
            ) {
                details = diagnostic;
            }
        }
        return Err(format!(
            "{} exited with {}: {}",
            formatter.command, output.status, details
        ));
    }
    let mut formatted = String::from_utf8(output.stdout)
        .map_err(|error| format!("formatter returned non-UTF-8 output: {error}"))?;
    for (sentinel, placeholder) in placeholders {
        formatted = formatted.replace(&sentinel, &placeholder);
    }
    Ok(normalize_formatter_output(input, formatted))
}

fn is_sqruff(command: &str) -> bool {
    Path::new(command)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("sqruff"))
}

fn sqruff_lint_diagnostic(
    command: &str,
    dialect: &str,
    input: &str,
    formatter_args: &[String],
) -> Option<String> {
    let mut args = vec![
        "lint".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--dialect".to_string(),
        dialect.to_string(),
    ];
    if let Some(config_index) = formatter_args.iter().position(|arg| arg == "--config") {
        if let Some(config) = formatter_args.get(config_index + 1) {
            args.extend(["--config".to_string(), config.clone()]);
        }
    }
    args.push("-".to_string());

    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.as_mut()?.write_all(input.as_bytes()).ok()?;
    let output = child.wait_with_output().ok()?;
    parse_sqruff_json_diagnostic(&output.stdout)
}

fn parse_sqruff_json_diagnostic(output: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(output).ok()?;
    let diagnostics = value.as_object()?.values().find_map(Value::as_array)?;
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.get("code").is_none_or(Value::is_null))
        .or_else(|| diagnostics.first())?;
    let line = diagnostic.pointer("/range/start/line")?.as_u64()?;
    let column = diagnostic.pointer("/range/start/character")?.as_u64()?;
    let message = diagnostic.get("message")?.as_str()?;
    let code = diagnostic
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("parse");
    Some(format!("L: {line} | P: {column} | {code} | {message}"))
}

fn formatter_error_details(stderr: &str, stdout: &str) -> String {
    let details = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if details.is_empty() {
        "formatter produced no error message".to_string()
    } else {
        details.chars().take(2000).collect()
    }
}

fn normalize_formatter_output(input: &str, formatted: String) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let core = formatted.trim_end_matches(['\r', '\n']);
    let is_multiline = core.contains('\n') || core.contains('\r');
    if is_multiline {
        format!("{core}{newline}")
    } else {
        core.to_string()
    }
}

fn apply_format_indent(formatted: &str, indent: &str) -> String {
    if indent.is_empty() {
        return formatted.to_string();
    }
    if !formatted.contains('\n') && !formatted.contains('\r') {
        return formatted.to_string();
    }

    let mut result =
        String::with_capacity(formatted.len() + formatted.lines().count() * indent.len());
    for line in formatted.split_inclusive('\n') {
        let content_end = line.trim_end_matches(['\r', '\n']).len();
        if content_end == 0 || line[..content_end].trim().is_empty() {
            result.push_str(line);
            continue;
        }
        result.push_str(indent);
        result.push_str(line);
    }
    result
}

fn mask_placeholders(input: &str, dialect: &str) -> (String, Vec<(String, String)>) {
    let bytes = input.as_bytes();
    let mut masked = String::with_capacity(input.len());
    let mut placeholders = Vec::new();
    let mut copied_until = 0;
    let mut index = 0;
    while index < bytes.len() {
        let named = match bytes[index] {
            b':' if bytes
                .get(index + 1)
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
                && (index == 0 || bytes[index - 1] != b':') =>
            {
                true
            }
            b'$' | b'@'
                if bytes
                    .get(index + 1)
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_') =>
            {
                true
            }
            b'?' if matches!(dialect, "mysql" | "mariadb" | "sqlite") => true,
            _ => false,
        };
        if !named {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        if bytes[index] != b'?' {
            end += 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'$'))
            {
                end += 1;
            }
        }
        if bytes[index] == b'$' && bytes.get(end) == Some(&b'$') {
            index = end + 1;
            continue;
        }
        let sentinel = format!("inline_sql_placeholder_{}", placeholders.len());
        masked.push_str(&input[copied_until..index]);
        masked.push_str(&sentinel);
        placeholders.push((sentinel, input[index..end].to_string()));
        copied_until = end;
        index = end;
    }
    masked.push_str(&input[copied_until..]);
    (masked, placeholders)
}

fn token_segments(text: &str, start: usize, end: usize) -> Vec<(u32, u32, u32)> {
    let mut result = Vec::new();
    let mut offset = start;
    while offset < end {
        let line_end = text[offset..end]
            .find('\n')
            .map(|relative| offset + relative)
            .unwrap_or(end);
        if line_end > offset {
            let (line, column) = position_tuple(text, offset);
            let length = text[offset..line_end].encode_utf16().count() as u32;
            result.push((line, column, length));
        }
        offset = line_end.saturating_add(1);
    }
    result
}

fn position(text: &str, offset: usize) -> Value {
    let (line, character) = position_tuple(text, offset);
    json!({ "line": line, "character": character })
}

fn position_tuple(text: &str, offset: usize) -> (u32, u32) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let character = text[line_start..offset].encode_utf16().count() as u32;
    (line, character)
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn respond(writer: &mut impl Write, id: Option<Value>, result: Value) -> io::Result<()> {
    write_message(
        writer,
        &json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result }),
    )
}

fn respond_error(
    writer: &mut impl Write,
    id: Option<Value>,
    code: i32,
    message: &str,
) -> io::Result<()> {
    write_message(
        writer,
        &json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "error": { "code": code, "message": message } }),
    )
}

fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn publish_diagnostics(
    writer: &mut impl Write,
    uri: &str,
    diagnostics: Vec<Value>,
) -> io::Result<()> {
    write_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": diagnostics }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_multiline_query_is_found_and_highlighted() {
        let document = Document {
            language_id: "go".into(),
            text: "const someNameQuery = `SELECT *\nFROM asdasd`".into(),
        };
        let settings = Settings::default();
        assert_eq!(injections(&document, &settings).len(), 1);
        assert!(!semantic_tokens(&document, &settings).is_empty());
    }

    #[test]
    fn go_sql_statement_without_query_suffix_is_found() {
        let document = Document {
            language_id: "go".into(),
            text: "const updateSystemModel = `\n\tUPDATE\n\t    users\n\tSET\n\t    name = :name\n\tWHERE\n\t    id = :id`"
                .into(),
        };
        let settings = Settings::default();
        assert_eq!(injections(&document, &settings).len(), 1);
        assert!(!semantic_tokens(&document, &settings).is_empty());
    }

    #[test]
    fn formatter_masking_preserves_named_placeholders_and_postgres_casts() {
        let input = "WHERE project = :my_column AND owner = $owner AND id = $1::UUID";
        let (masked, placeholders) = mask_placeholders(input, "postgres");
        assert_eq!(
            masked,
            "WHERE project = inline_sql_placeholder_0 AND owner = inline_sql_placeholder_1 AND id = $1::UUID"
        );
        let restored = placeholders
            .into_iter()
            .fold(masked, |text, (sentinel, placeholder)| {
                text.replace(&sentinel, &placeholder)
            });
        assert_eq!(restored, input);
    }

    #[test]
    fn single_line_formatter_output_keeps_closing_delimiter_inline() {
        assert_eq!(
            normalize_formatter_output("select id from users", "SELECT id FROM users\n\n".into()),
            "SELECT id FROM users"
        );
    }

    #[test]
    fn multiline_formatter_output_has_exactly_one_final_newline() {
        assert_eq!(
            normalize_formatter_output(
                "\nselect id\nfrom users",
                "\nSELECT id\nFROM users\n\n".into()
            ),
            "\nSELECT id\nFROM users\n"
        );
    }

    #[test]
    fn format_indent_is_applied_to_non_empty_sql_lines() {
        assert_eq!(
            apply_format_indent(
                "\nINSERT INTO locks (lock_name, owner, expires_at)\n    VALUES ($1, NULL, NULL)\n",
                "\t"
            ),
            "\n\tINSERT INTO locks (lock_name, owner, expires_at)\n\t    VALUES ($1, NULL, NULL)\n"
        );
    }

    #[test]
    fn format_indent_is_not_applied_to_single_line_sql() {
        assert_eq!(
            apply_format_indent("SELECT id FROM users", "\t"),
            "SELECT id FROM users"
        );
    }

    #[test]
    fn formatter_errors_prefer_stderr_and_fall_back_to_stdout() {
        assert_eq!(
            formatter_error_details("bad syntax\n", "ignored"),
            "bad syntax"
        );
        assert_eq!(
            formatter_error_details("", "parse failed\n"),
            "parse failed"
        );
        assert_eq!(
            formatter_error_details("", ""),
            "formatter produced no error message"
        );
    }

    #[test]
    fn formatter_error_locations_resolve_to_the_exact_sql_line() {
        assert_eq!(formatter_error_line("L:   3 | P: 7 | parse error"), Some(3));
        assert_eq!(formatter_error_line("error at line 4, column 2"), Some(4));

        let document = Document {
            language_id: "go".into(),
            text: "const q = `SELECT\ninvalid syntax\nFROM users`".into(),
        };
        let start = document.text.find("SELECT").unwrap();
        let end = document.text[start..]
            .find('`')
            .map(|offset| start + offset)
            .unwrap_or(document.text.len());
        let range = sql_line_range(&document, Injection { start, end }, 2).unwrap();
        assert_eq!(&document.text[range.start..range.end], "invalid syntax");
    }

    #[test]
    fn parses_sqruff_json_diagnostic_locations() {
        let output = br#"{"<string>":[{"range":{"start":{"line":3,"character":6},"end":{"line":3,"character":6}},"message":"Couldn't find closing bracket.","severity":"Warning","source":"sqruff","code":null}]}"#;
        let diagnostic = parse_sqruff_json_diagnostic(output).unwrap();
        assert_eq!(
            diagnostic,
            "L: 3 | P: 6 | parse | Couldn't find closing bracket."
        );
        assert_eq!(formatter_error_line(&diagnostic), Some(3));
    }
}
