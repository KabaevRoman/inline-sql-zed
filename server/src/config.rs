use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub patterns: HashMap<String, Vec<String>>,
    pub dialect: String,
    pub dialect_by_language: HashMap<String, String>,
    pub formatter: Option<String>,
    pub formatter_by_language: HashMap<String, String>,
    pub format_indent: String,
    pub format_indent_by_language: HashMap<String, String>,
    pub formatters: HashMap<String, FormatterCommand>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FormatterCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert(
            "go".into(),
            vec![
                r"(?s)\b(?:const|var)\s+\w*(?:Query|SQL)\w*\s*=\s*`(?P<sql>.*?)`".into(),
                r"(?is)\b(?:const|var)\s+\w+\s*=\s*`(?P<sql>\s*(?:SELECT|INSERT|UPDATE|DELETE|WITH|MERGE|CREATE|ALTER|DROP|TRUNCATE)\b.*?)`".into(),
                r#"(?s)\b(?:const|var)\s+\w*(?:Query|SQL)\w*\s*=\s*\"(?P<sql>(?:\\.|[^\"\\])*)\""#
                    .into(),
            ],
        );
        patterns.insert(
            "rust".into(),
            vec![
                r#"(?s)\blet\s+(?:mut\s+)?\w*(?:query|sql)\w*\s*(?::[^=]+)?=\s*r?\"(?P<sql>.*?)\""#
                    .into(),
                r#"(?s)\b(?:query|execute|query_as)!\s*\(\s*r?\"(?P<sql>.*?)\""#.into(),
            ],
        );
        for language in ["javascript", "typescript", "typescriptreact"] {
            patterns.insert(
                language.into(),
                vec![
                    r"(?s)\b(?:const|let|var)\s+\w*(?:Query|SQL)\w*\s*=\s*`(?P<sql>.*?)`".into(),
                    r"(?s)\bsql\s*`(?P<sql>.*?)`".into(),
                ],
            );
        }
        patterns.insert(
            "python".into(),
            vec![
                r#"(?s)\b\w*(?:query|sql)\w*\s*=\s*(?:[rubfRUBF]*)\"\"\"(?P<sql>.*?)\"\"\""#.into(),
                r"(?s)\b\w*(?:query|sql)\w*\s*=\s*(?:[rubfRUBF]*)'''(?P<sql>.*?)'''".into(),
            ],
        );
        patterns.insert(
            "java".into(),
            vec![
                r#"(?s)\b(?:String|var)\s+\w*(?:Query|SQL)\w*\s*=\s*\"\"\"(?P<sql>.*?)\"\"\""#
                    .into(),
                r#"(?s)\bString\s+\w*(?:Query|SQL)\w*\s*=\s*\"(?P<sql>(?:\\.|[^\"\\])*)\""#.into(),
            ],
        );
        patterns.insert(
            "kotlin".into(),
            vec![
                r#"(?s)\b(?:val|var)\s+\w*(?:Query|SQL)\w*\s*=\s*\"\"\"(?P<sql>.*?)\"\"\""#.into(),
            ],
        );
        patterns.insert(
            "csharp".into(),
            vec![
                r#"(?s)\b(?:const\s+)?string\s+\w*(?:Query|Sql|SQL)\w*\s*=\s*@\"(?P<sql>.*?)\""#
                    .into(),
            ],
        );
        patterns.insert(
            "ruby".into(),
            vec![r"(?s)\b\w*(?:query|sql)\w*\s*=\s*<<[-~]?SQL\s*\n(?P<sql>.*?)\nSQL".into()],
        );
        patterns.insert(
            "php".into(),
            vec![
                r#"(?s)\$\w*(?:Query|Sql|SQL)\w*\s*=\s*<<<['"]?SQL['"]?\s*\n(?P<sql>.*?)\nSQL"#
                    .into(),
            ],
        );
        patterns.insert(
            "elixir".into(),
            vec![r#"(?s)\b\w*(?:query|sql)\w*\s*=\s*\"\"\"(?P<sql>.*?)\"\"\""#.into()],
        );
        for language in ["scala", "swift"] {
            patterns.insert(language.into(), vec![
                r#"(?s)\b(?:val|var|let)\s+\w*(?:Query|Sql|SQL)\w*(?:\s*:\s*\w+)?\s*=\s*\"\"\"(?P<sql>.*?)\"\"\""#.into(),
            ]);
        }
        for language in ["c", "cpp"] {
            patterns.insert(language.into(), vec![
                r#"(?s)\b(?:const\s+)?char\s*\*?\s*\w*(?:Query|Sql|SQL)\w*\s*=\s*\"(?P<sql>(?:\\.|[^\"\\])*)\""#.into(),
            ]);
        }

        let formatters = HashMap::from([
            (
                "sqlfluff".into(),
                FormatterCommand {
                    command: "sqlfluff".into(),
                    args: vec![
                        "format".into(),
                        "--dialect".into(),
                        "{dialect}".into(),
                        "-".into(),
                    ],
                },
            ),
            (
                "sqruff".into(),
                FormatterCommand {
                    command: "sqruff".into(),
                    args: vec![
                        "fix".into(),
                        "--format".into(),
                        "json".into(),
                        "--dialect".into(),
                        "{dialect}".into(),
                        "-".into(),
                    ],
                },
            ),
        ]);
        Self {
            patterns,
            dialect: "postgres".into(),
            dialect_by_language: HashMap::new(),
            formatter: None,
            formatter_by_language: HashMap::new(),
            format_indent: String::new(),
            format_indent_by_language: HashMap::new(),
            formatters,
        }
    }
}

impl Settings {
    pub fn update(&mut self, value: Value) {
        let value = value
            .get("inline-sql")
            .or_else(|| value.get("inline_sql"))
            .cloned()
            .unwrap_or(value);
        if value.is_null() {
            return;
        }
        if let Ok(replacement) = serde_json::from_value::<Settings>(value) {
            *self = replacement;
        }
    }

    pub fn patterns_for(&self, language_id: &str) -> &[String] {
        let normalized = normalize(language_id);
        self.patterns
            .iter()
            .find(|(language, _)| normalize(language) == normalized)
            .map(|(_, patterns)| patterns.as_slice())
            .unwrap_or(&[])
    }

    pub fn formatter_for(&self, language_id: &str) -> Result<Option<&FormatterCommand>, String> {
        let normalized = normalize(language_id);
        let Some(selected) = self
            .formatter_by_language
            .iter()
            .find(|(language, _)| normalize(language) == normalized)
            .map(|(_, formatter)| formatter)
            .or(self.formatter.as_ref())
        else {
            return Ok(None);
        };
        self.formatters.get(selected).map(Some).ok_or_else(|| {
            format!(
                "formatter '{selected}' is selected but is not defined under lsp.inline-sql.settings.formatters"
            )
        })
    }

    pub fn dialect_for(&self, language_id: &str) -> String {
        let normalized = normalize(language_id);
        let dialect = self
            .dialect_by_language
            .iter()
            .find(|(language, _)| normalize(language) == normalized)
            .map(|(_, dialect)| dialect.as_str())
            .unwrap_or(&self.dialect);
        canonical_dialect(dialect).to_string()
    }

    pub fn format_indent_for(&self, language_id: &str) -> &str {
        let normalized = normalize(language_id);
        self.format_indent_by_language
            .iter()
            .find(|(language, _)| normalize(language) == normalized)
            .map(|(_, indent)| indent.as_str())
            .unwrap_or(&self.format_indent)
    }
}

fn canonical_dialect(value: &str) -> &str {
    match normalize(value).as_str() {
        "postgres" | "postgresql" | "psql" | "pg" => "postgres",
        "mssql" | "sqlserver" | "tsql" => "tsql",
        "maria" | "mariadb" => "mariadb",
        "sqlite" | "sqlite3" => "sqlite",
        "bigquery" | "bq" => "bigquery",
        "snowflake" => "snowflake",
        "redshift" => "redshift",
        "duckdb" => "duckdb",
        "oracle" | "plsql" => "oracle",
        "mysql" => "mysql",
        "clickhouse" => "clickhouse",
        "ansi" | "generic" => "ansi",
        _ => value,
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partial_settings_keep_default_patterns() {
        let mut settings = Settings::default();
        settings.update(json!({ "formatter": "sqruff" }));
        assert!(!settings.patterns_for("Go").is_empty());
        assert_eq!(
            settings.formatter_for("go").unwrap().unwrap().command,
            "sqruff"
        );
        assert_eq!(
            settings.formatter_for("go").unwrap().unwrap().args,
            ["fix", "--format", "json", "--dialect", "{dialect}", "-"]
        );
        assert_eq!(settings.dialect_for("go"), "postgres");
    }

    #[test]
    fn custom_patterns_replace_defaults_and_allow_multiple_rules() {
        let mut settings = Settings::default();
        settings.update(json!({ "patterns": { "Go": ["one", "two"] } }));
        assert_eq!(settings.patterns_for("go"), ["one", "two"]);
        assert!(settings.patterns_for("python").is_empty());
    }

    #[test]
    fn dialect_aliases_and_language_overrides_are_normalized() {
        let mut settings = Settings::default();
        settings.update(json!({
            "dialect": "psql",
            "dialect_by_language": { "TypeScript": "sqlite3" }
        }));
        assert_eq!(settings.dialect_for("go"), "postgres");
        assert_eq!(settings.dialect_for("typescript"), "sqlite");
    }

    #[test]
    fn format_indent_overrides_are_normalized() {
        let mut settings = Settings::default();
        settings.update(json!({
            "format_indent": "  ",
            "format_indent_by_language": { "Go": "\t" }
        }));
        assert_eq!(settings.format_indent_for("go"), "\t");
        assert_eq!(settings.format_indent_for("python"), "  ");
    }

    #[test]
    fn reports_an_unknown_selected_formatter() {
        let mut settings = Settings::default();
        settings.update(json!({ "formatter": "missing" }));
        assert!(settings
            .formatter_for("go")
            .unwrap_err()
            .contains("missing"));
    }
}
