use regex::Regex;
use std::sync::LazyLock;

use crate::statements::Statement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Style,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Severity::Style => "STYLE",
            Severity::Warning => "WARNING",
            Severity::Error => "ERROR",
        })
    }
}

pub struct Finding {
    pub line: usize,
    pub severity: Severity,
    pub rule: &'static str,
    pub message: String,
}

static SELECT_STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bselect\s+\*").unwrap());
static WHERE_KW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bwhere\b").unwrap());
static UPDATE_DELETE_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(update|delete)\b").unwrap());
static NULL_EQUALITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(=|<>|!=)\s*null\b|\bnull\s*(=|<>|!=)").unwrap());
static NOT_IN_SUBQUERY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bnot\s+in\s*\(\s*select\b").unwrap());
// `(?:\s+\w+)?` after each table name absorbs an optional alias
// (`FROM accounts a, billing b`) — without it, the common case of
// aliased tables in an implicit join slipped past undetected.
static IMPLICIT_JOIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bfrom\s+[\w.]+(?:\s+(?:as\s+)?\w+)?\s*,\s*[\w.]+(?:\s+(?:as\s+)?\w+)?")
        .unwrap()
});
static HAS_JOIN_KW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bjoin\b").unwrap());
static DESTRUCTIVE_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(drop|truncate)\b").unwrap());

fn line_of(masked_statement: &str, base_line: usize, match_start: usize) -> usize {
    base_line + masked_statement[..match_start].matches('\n').count()
}

/// Runs every statement-scoped rule against one statement, given both
/// its masked text (for keyword matching) and its base line number.
pub fn check_statement(stmt: &Statement, masked: &str) -> Vec<Finding> {
    let raw = stmt.text(masked);
    let base_line = stmt.start_line(masked);
    // `base_line` is already the line of the first non-whitespace byte
    // (see `Statement::start_line`), so matching against `raw` directly
    // and counting its newlines up to a match would double-count any
    // blank lines between statements: once implicitly via `base_line`,
    // once explicitly by `line_of`. Trimming here keeps both counts
    // anchored to the same starting point.
    let leading_ws = raw.len() - raw.trim_start().len();
    let text = &raw[leading_ws..];
    let mut findings = Vec::new();

    if let Some(m) = SELECT_STAR.find(text) {
        findings.push(Finding {
            line: line_of(text, base_line, m.start()),
            severity: Severity::Warning,
            rule: "select-star",
            message: "SELECT * pulls every column, including ones added later — prefer an explicit column list".to_string(),
        });
    }

    if UPDATE_DELETE_START.is_match(text) && !WHERE_KW.is_match(text) {
        let verb = if text.trim_start().to_lowercase().starts_with("update") {
            "UPDATE"
        } else {
            "DELETE"
        };
        findings.push(Finding {
            line: base_line,
            severity: Severity::Error,
            rule: "no-where",
            message: format!("{verb} with no WHERE clause — this affects every row in the table"),
        });
    }

    if let Some(m) = NULL_EQUALITY.find(text) {
        findings.push(Finding {
            line: line_of(text, base_line, m.start()),
            severity: Severity::Warning,
            rule: "null-equality",
            message: "`= NULL` / `<> NULL` is always UNKNOWN in SQL, never TRUE — use IS NULL / IS NOT NULL".to_string(),
        });
    }

    if let Some(m) = NOT_IN_SUBQUERY.find(text) {
        findings.push(Finding {
            line: line_of(text, base_line, m.start()),
            severity: Severity::Warning,
            rule: "not-in-subquery",
            message: "NOT IN (subquery): if the subquery returns even one NULL, the whole condition matches nothing — prefer NOT EXISTS".to_string(),
        });
    }

    if let Some(m) = IMPLICIT_JOIN.find(text) {
        if !HAS_JOIN_KW.is_match(text) && !WHERE_KW.is_match(text) {
            findings.push(Finding {
                line: line_of(text, base_line, m.start()),
                severity: Severity::Warning,
                rule: "implicit-cross-join",
                message: "comma-separated tables in FROM with no WHERE or JOIN — if there's no join condition anywhere, this is a full cartesian product".to_string(),
            });
        }
    }

    if let Some(m) = DESTRUCTIVE_START.find(text) {
        findings.push(Finding {
            line: line_of(text, base_line, m.start()),
            severity: Severity::Style,
            rule: "destructive-statement",
            message: "DROP/TRUNCATE — destructive and typically not transactional across all databases; make sure this is intentional".to_string(),
        });
    }

    findings
}

/// File-scoped style rules that operate on the raw (unmasked) text
/// line-by-line — trailing whitespace and indentation are about the
/// literal bytes on disk, not SQL semantics, so masking doesn't apply.
pub fn check_file_style(source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        if line.ends_with(' ') || line.ends_with('\t') {
            findings.push(Finding {
                line: line_no,
                severity: Severity::Style,
                rule: "trailing-whitespace",
                message: "trailing whitespace".to_string(),
            });
        }

        let leading: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if let Some(first_tab) = leading.find('\t') {
            if leading[..first_tab].contains(' ') {
                findings.push(Finding {
                    line: line_no,
                    severity: Severity::Style,
                    rule: "mixed-indentation",
                    message: "space(s) before a tab in leading indentation".to_string(),
                });
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask::mask_literals;
    use crate::statements::split_statements;

    fn findings_for(sql: &str) -> Vec<Finding> {
        let masked = mask_literals(sql);
        split_statements(&masked)
            .iter()
            .flat_map(|s| check_statement(s, &masked))
            .collect()
    }

    #[test]
    fn flags_select_star() {
        let f = findings_for("SELECT * FROM orders;");
        assert!(f.iter().any(|x| x.rule == "select-star"));
    }

    #[test]
    fn does_not_flag_count_star() {
        let f = findings_for("SELECT COUNT(*) FROM orders;");
        assert!(!f.iter().any(|x| x.rule == "select-star"));
    }

    #[test]
    fn flags_delete_without_where() {
        let f = findings_for("DELETE FROM users;");
        assert!(f
            .iter()
            .any(|x| x.rule == "no-where" && x.severity == Severity::Error));
    }

    #[test]
    fn does_not_flag_delete_with_where() {
        let f = findings_for("DELETE FROM users WHERE id = 1;");
        assert!(!f.iter().any(|x| x.rule == "no-where"));
    }

    #[test]
    fn flags_update_without_where() {
        let f = findings_for("UPDATE users SET active = false;");
        assert!(f.iter().any(|x| x.rule == "no-where"));
    }

    #[test]
    fn flags_null_equality() {
        let f = findings_for("SELECT * FROM t WHERE deleted_at = NULL;");
        assert!(f.iter().any(|x| x.rule == "null-equality"));
    }

    #[test]
    fn flags_not_in_subquery() {
        let f = findings_for("SELECT id FROM a WHERE id NOT IN (SELECT id FROM b);");
        assert!(f.iter().any(|x| x.rule == "not-in-subquery"));
    }

    #[test]
    fn flags_implicit_cross_join() {
        let f = findings_for("SELECT * FROM a, b;");
        assert!(f.iter().any(|x| x.rule == "implicit-cross-join"));
    }

    #[test]
    fn does_not_flag_comma_list_with_a_where_join_condition() {
        let f = findings_for("SELECT * FROM a, b WHERE a.id = b.a_id;");
        assert!(!f.iter().any(|x| x.rule == "implicit-cross-join"));
    }

    #[test]
    fn flags_implicit_cross_join_with_table_aliases() {
        let f = findings_for("SELECT a.id, b.name FROM accounts a, billing b;");
        assert!(f.iter().any(|x| x.rule == "implicit-cross-join"));
    }

    #[test]
    fn line_numbers_do_not_double_count_blank_lines_between_statements() {
        let sql = "SELECT 1;\n\nSELECT 2;\n\nSELECT * FROM t;";
        let f = findings_for(sql);
        let star = f.iter().find(|x| x.rule == "select-star").unwrap();
        assert_eq!(star.line, 5);
    }

    #[test]
    fn flags_drop_and_truncate_as_style_heads_up() {
        let f = findings_for("DROP TABLE users;");
        assert!(f
            .iter()
            .any(|x| x.rule == "destructive-statement" && x.severity == Severity::Style));
    }

    #[test]
    fn clean_statement_has_no_findings() {
        let f = findings_for("SELECT id, name FROM users WHERE active = true;");
        assert!(f.is_empty());
    }

    #[test]
    fn file_style_flags_trailing_whitespace_and_mixed_indentation() {
        let source = "SELECT 1;   \n \tSELECT 2;\n";
        let findings = check_file_style(source);
        assert!(findings
            .iter()
            .any(|f| f.rule == "trailing-whitespace" && f.line == 1));
        assert!(findings
            .iter()
            .any(|f| f.rule == "mixed-indentation" && f.line == 2));
    }
}
