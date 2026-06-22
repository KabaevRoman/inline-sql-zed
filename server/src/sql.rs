pub const TOKEN_TYPES: &[&str] = &[
    "keyword",
    "string",
    "number",
    "comment",
    "operator",
    "function",
    "type",
    "variable",
    "property",
    "parameter",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: u32,
}

pub fn tokenize(source: &str, dialect: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b'$' if dialect == "postgres"
                && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) =>
            {
                index += 2;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 8,
                });
            }
            b'$' if dialect == "postgres" => {
                if let Some(end) = postgres_dollar_string_end(source, index) {
                    index = end;
                    tokens.push(Token {
                        start,
                        end: index,
                        kind: 1,
                    });
                } else if bytes
                    .get(index + 1)
                    .is_some_and(|byte| is_identifier_start(*byte))
                {
                    index += 2;
                    while index < bytes.len() && is_identifier_continue(bytes[index]) {
                        index += utf8_width(bytes[index]);
                    }
                    tokens.push(Token {
                        start,
                        end: index,
                        kind: 8,
                    });
                } else {
                    index += 1;
                }
            }
            b':' if bytes
                .get(index + 1)
                .is_some_and(|byte| is_identifier_start(*byte))
                && (index == 0 || bytes[index - 1] != b':') =>
            {
                index += 2;
                while index < bytes.len() && is_identifier_continue(bytes[index]) {
                    index += utf8_width(bytes[index]);
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 8,
                });
            }
            b'@' if dialect == "tsql"
                && bytes
                    .get(index + 1)
                    .is_some_and(|byte| is_identifier_start(*byte)) =>
            {
                index += 2;
                while index < bytes.len() && is_identifier_continue(bytes[index]) {
                    index += utf8_width(bytes[index]);
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 8,
                });
            }
            b'?' if matches!(dialect, "mysql" | "mariadb" | "sqlite") => {
                index += 1;
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 8,
                });
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 3,
                });
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += utf8_width(bytes[index]);
                }
                index = (index + 2).min(bytes.len());
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 3,
                });
            }
            b'\'' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\'' {
                        index += 1;
                        if bytes.get(index) == Some(&b'\'') {
                            index += 1;
                        } else {
                            break;
                        }
                    } else {
                        index += utf8_width(bytes[index]);
                    }
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 1,
                });
            }
            b'"' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == quote {
                        index += 1;
                        break;
                    }
                    index += utf8_width(bytes[index]);
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 7,
                });
            }
            byte if byte.is_ascii_digit() => {
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || b"._".contains(&bytes[index]))
                {
                    index += 1;
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 2,
                });
            }
            byte if is_identifier_start(byte) => {
                index += utf8_width(byte);
                while index < bytes.len() && is_identifier_continue(bytes[index]) {
                    index += utf8_width(bytes[index]);
                }
                let word = source[start..index].to_ascii_uppercase();
                let kind = if is_keyword(&word, dialect) {
                    0
                } else if is_type(&word, dialect) {
                    6
                } else if next_non_whitespace(bytes, index) == Some(b'(') {
                    5
                } else {
                    7
                };
                tokens.push(Token {
                    start,
                    end: index,
                    kind,
                });
            }
            byte if b"+-*/%=<>!|&^~.,;:()[]".contains(&byte) => {
                index += 1;
                while index < bytes.len() && b"=<>|&".contains(&bytes[index]) {
                    index += 1;
                }
                tokens.push(Token {
                    start,
                    end: index,
                    kind: 4,
                });
            }
            byte => index += utf8_width(byte),
        }
    }
    mark_aliases(source, &mut tokens);
    tokens
}

fn postgres_dollar_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut delimiter_end = start + 1;
    while delimiter_end < bytes.len()
        && (bytes[delimiter_end].is_ascii_alphanumeric() || bytes[delimiter_end] == b'_')
    {
        delimiter_end += 1;
    }
    if bytes.get(delimiter_end) != Some(&b'$') {
        return None;
    }
    let delimiter = &source[start..=delimiter_end];
    source[delimiter_end + 1..]
        .find(delimiter)
        .map(|relative| delimiter_end + 1 + relative + delimiter.len())
}

fn mark_aliases(source: &str, tokens: &mut [Token]) {
    for index in 0..tokens.len() {
        if token_text(source, tokens[index]).eq_ignore_ascii_case("AS") {
            if let Some(alias) = next_code_token(tokens, index + 1) {
                if tokens[alias].kind == 7 {
                    tokens[alias].kind = 8;
                }
            }
        }
        if token_text(source, tokens[index]).eq_ignore_ascii_case("WITH") {
            if let Some(alias) = next_code_token(tokens, index + 1) {
                if tokens[alias].kind == 7 {
                    tokens[alias].kind = 8;
                }
            }
        }
        if ["FROM", "JOIN", "UPDATE", "INTO"]
            .iter()
            .any(|keyword| token_text(source, tokens[index]).eq_ignore_ascii_case(keyword))
        {
            mark_implicit_table_alias(source, tokens, index + 1);
        }
    }

    let aliases = tokens
        .iter()
        .filter(|token| token.kind == 8)
        .map(|token| unquote_identifier(token_text(source, *token)).to_ascii_lowercase())
        .collect::<Vec<_>>();
    for token in tokens {
        if token.kind == 7
            && aliases.iter().any(|alias| {
                unquote_identifier(token_text(source, *token)).eq_ignore_ascii_case(alias)
            })
        {
            token.kind = 8;
        }
    }
}

fn unquote_identifier(identifier: &str) -> &str {
    identifier
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            identifier
                .strip_prefix('`')
                .and_then(|value| value.strip_suffix('`'))
        })
        .unwrap_or(identifier)
}

fn mark_implicit_table_alias(source: &str, tokens: &mut [Token], start: usize) {
    let Some(mut current) = next_code_token(tokens, start) else {
        return;
    };
    if tokens[current].kind != 7 {
        return;
    }
    loop {
        let Some(next) = next_code_token(tokens, current + 1) else {
            return;
        };
        if tokens[next].kind == 4 && token_text(source, tokens[next]) == "." {
            let Some(identifier) = next_code_token(tokens, next + 1) else {
                return;
            };
            if tokens[identifier].kind == 7 {
                current = identifier;
                continue;
            }
        }
        if tokens[next].kind == 7 {
            tokens[next].kind = 8;
        }
        return;
    }
}

fn next_code_token(tokens: &[Token], mut index: usize) -> Option<usize> {
    while index < tokens.len() && tokens[index].kind == 3 {
        index += 1;
    }
    (index < tokens.len()).then_some(index)
}

fn token_text(source: &str, token: Token) -> &str {
    &source[token.start..token.end]
}

fn next_non_whitespace(bytes: &[u8], mut index: usize) -> Option<u8> {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    bytes.get(index).copied()
}

fn utf8_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

fn is_keyword(word: &str, dialect: &str) -> bool {
    KEYWORDS.contains(&word)
        || match dialect {
            "postgres" => POSTGRES_KEYWORDS.contains(&word),
            "mysql" | "mariadb" => MYSQL_KEYWORDS.contains(&word),
            "sqlite" => SQLITE_KEYWORDS.contains(&word),
            "bigquery" => BIGQUERY_KEYWORDS.contains(&word),
            "snowflake" | "redshift" => SNOWFLAKE_KEYWORDS.contains(&word),
            "tsql" => TSQL_KEYWORDS.contains(&word),
            "oracle" => ORACLE_KEYWORDS.contains(&word),
            "duckdb" => DUCKDB_KEYWORDS.contains(&word),
            "clickhouse" => CLICKHOUSE_KEYWORDS.contains(&word),
            _ => false,
        }
}

fn is_type(word: &str, dialect: &str) -> bool {
    TYPES.contains(&word)
        || match dialect {
            "postgres" => POSTGRES_TYPES.contains(&word),
            "mysql" | "mariadb" => MYSQL_TYPES.contains(&word),
            "bigquery" => BIGQUERY_TYPES.contains(&word),
            "snowflake" | "redshift" => SNOWFLAKE_TYPES.contains(&word),
            "tsql" => TSQL_TYPES.contains(&word),
            "oracle" => ORACLE_TYPES.contains(&word),
            "clickhouse" => CLICKHOUSE_TYPES.contains(&word),
            _ => false,
        }
}

const KEYWORDS: &[&str] = &[
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASE",
    "CHECK",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IF",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LIKE",
    "LIMIT",
    "MERGE",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRIMARY",
    "PROCEDURE",
    "REFERENCES",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TRIGGER",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

const TYPES: &[&str] = &[
    "BIGINT",
    "BINARY",
    "BIT",
    "BLOB",
    "BOOLEAN",
    "CHAR",
    "DATE",
    "DATETIME",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "INT",
    "INTEGER",
    "INTERVAL",
    "JSON",
    "NUMERIC",
    "REAL",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "UUID",
    "VARCHAR",
];

const POSTGRES_KEYWORDS: &[&str] = &[
    "ANALYZE",
    "CONFLICT",
    "FILTER",
    "ILIKE",
    "LATERAL",
    "MATERIALIZED",
    "NOTHING",
    "NULLS",
    "ONLY",
    "SIMILAR",
    "VACUUM",
    "VERBOSE",
];
const POSTGRES_TYPES: &[&str] = &[
    "ARRAY",
    "BIGSERIAL",
    "BYTEA",
    "CIDR",
    "INET",
    "JSONB",
    "MONEY",
    "SERIAL",
    "TIMESTAMPTZ",
    "TSQUERY",
    "TSVECTOR",
    "XML",
];
const MYSQL_KEYWORDS: &[&str] = &[
    "AUTO_INCREMENT",
    "DESCRIBE",
    "ENGINE",
    "LOCK",
    "REPLACE",
    "SHOW",
    "UNLOCK",
    "USE",
];
const MYSQL_TYPES: &[&str] = &["ENUM", "LONGBLOB", "LONGTEXT", "MEDIUMINT", "TINYINT"];
const SQLITE_KEYWORDS: &[&str] = &["ATTACH", "DETACH", "GLOB", "PRAGMA", "WITHOUT"];
const BIGQUERY_KEYWORDS: &[&str] = &["QUALIFY", "STRUCT", "UNNEST"];
const BIGQUERY_TYPES: &[&str] = &["ARRAY", "BIGNUMERIC", "BYTES", "GEOGRAPHY", "STRUCT"];
const SNOWFLAKE_KEYWORDS: &[&str] = &["CLONE", "QUALIFY", "SAMPLE", "WAREHOUSE"];
const SNOWFLAKE_TYPES: &[&str] = &["ARRAY", "OBJECT", "VARIANT"];
const TSQL_KEYWORDS: &[&str] = &["GO", "IDENTITY", "MERGE", "OUTPUT", "TOP"];
const TSQL_TYPES: &[&str] = &["IMAGE", "NCHAR", "NTEXT", "NVARCHAR", "UNIQUEIDENTIFIER"];
const ORACLE_KEYWORDS: &[&str] = &["CONNECT", "MINUS", "MODEL", "SIBLINGS", "START"];
const ORACLE_TYPES: &[&str] = &["BFILE", "CLOB", "NCHAR", "NCLOB", "NUMBER", "VARCHAR2"];
const DUCKDB_KEYWORDS: &[&str] = &["EXCLUDE", "PIVOT", "QUALIFY", "SAMPLE", "UNPIVOT"];
const CLICKHOUSE_KEYWORDS: &[&str] = &["FINAL", "FORMAT", "PREWHERE", "SAMPLE"];
const CLICKHOUSE_TYPES: &[&str] = &["FIXEDSTRING", "LOWCARDINALITY", "TUPLE", "UINT64"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_basic_sql() {
        let source = "SELECT id, count(*) FROM users WHERE id = 42 -- user";
        let tokens = tokenize(source, "ansi");
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "SELECT" && token.kind == 0));
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "count" && token.kind == 5));
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "42" && token.kind == 2));
    }

    #[test]
    fn highlights_explicit_and_implicit_aliases() {
        let source = "SELECT u.id AS user_id FROM public.users u";
        let tokens = tokenize(source, "postgres");
        let aliases = tokens
            .iter()
            .filter(|token| token.kind == 8)
            .map(|token| &source[token.start..token.end])
            .collect::<Vec<_>>();
        assert_eq!(aliases, ["u", "user_id", "u"]);
    }

    #[test]
    fn recognizes_postgres_syntax() {
        let source = "SELECT $1::JSONB, $$text$$ FROM events WHERE name ILIKE '%zed%'";
        let tokens = tokenize(source, "postgres");
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "$1" && token.kind == 8));
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "$$text$$" && token.kind == 1));
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "JSONB" && token.kind == 6));
    }

    #[test]
    fn highlights_alias_references_and_postgres_placeholders() {
        let source = "SELECT um.id, sm.name FROM user_models um JOIN system_models sm ON um.model_id = sm.id WHERE um.project = $1";
        let tokens = tokenize(source, "postgres");
        let alias_references = tokens
            .iter()
            .filter(|token| token.kind == 8)
            .map(|token| &source[token.start..token.end])
            .collect::<Vec<_>>();
        assert_eq!(
            alias_references,
            ["um", "sm", "um", "sm", "um", "sm", "um", "$1"]
        );
        assert!(tokens
            .iter()
            .any(|token| &source[token.start..token.end] == "$1" && token.kind == 8));
    }

    #[test]
    fn highlights_named_and_dialect_placeholders() {
        let postgres = "WHERE project = :my_column AND owner = $owner AND id = $1";
        let postgres_tokens = tokenize(postgres, "postgres");
        for placeholder in [":my_column", "$owner", "$1"] {
            assert!(postgres_tokens.iter().any(|token| {
                &postgres[token.start..token.end] == placeholder && token.kind == 8
            }));
        }

        let tsql = "WHERE project = @project";
        assert!(tokenize(tsql, "tsql")
            .iter()
            .any(|token| &tsql[token.start..token.end] == "@project" && token.kind == 8));

        let sqlite = "WHERE project = ?";
        assert!(tokenize(sqlite, "sqlite")
            .iter()
            .any(|token| &sqlite[token.start..token.end] == "?" && token.kind == 8));
    }
}
