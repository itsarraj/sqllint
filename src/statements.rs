/// One `;`-delimited statement, as a byte range into the original
/// (unmasked) source — sliceable directly from either the original or
/// masked text since both are the same length.
pub struct Statement {
    pub start: usize,
    pub end: usize,
}

impl Statement {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }

    /// 1-indexed line number of the statement's first substantive
    /// character. Takes `masked` text (not the original source)
    /// specifically so a leading comment (`-- ...`, blanked out by
    /// `mask::mask_literals` into plain spaces) is skipped along with
    /// real whitespace — trimming against the original source would
    /// stop at the comment's first `-` and misreport the comment's own
    /// line instead of the statement's. `start` itself also often sits
    /// right after the previous statement's `;`, still on the previous
    /// line, before the newline that separates the two — so counting
    /// from `start` directly, without trimming, would additionally
    /// under-report by one line whenever a statement begins on its own
    /// line (the common case).
    pub fn start_line(&self, masked: &str) -> usize {
        let text = &masked[self.start..self.end];
        let leading_ws = text.len() - text.trim_start().len();
        1 + masked[..self.start + leading_ws].matches('\n').count()
    }
}

/// Splits masked text into statements at top-level `;` characters. Since
/// semicolons inside strings/comments were already blanked out by
/// `mask::mask_literals`, every `;` remaining here is a real statement
/// terminator — no separate quote-tracking needed at this stage. A
/// trailing statement with no closing `;` (common, and valid SQL) is
/// still included.
pub fn split_statements(masked: &str) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut start = 0;

    for (idx, _) in masked.match_indices(';') {
        let end = idx + 1;
        if masked[start..end].trim().len() > 1 {
            statements.push(Statement { start, end });
        }
        start = end;
    }
    if !masked[start..].trim().is_empty() {
        statements.push(Statement {
            start,
            end: masked.len(),
        });
    }

    statements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask::mask_literals;

    #[test]
    fn splits_on_semicolons() {
        let sql = "SELECT 1; SELECT 2;";
        let masked = mask_literals(sql);
        let stmts = split_statements(&masked);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0].text(sql).trim(), "SELECT 1;");
        assert_eq!(stmts[1].text(sql).trim(), "SELECT 2;");
    }

    #[test]
    fn trailing_statement_without_semicolon_is_kept() {
        let sql = "SELECT 1;\nSELECT 2";
        let masked = mask_literals(sql);
        let stmts = split_statements(&masked);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[1].text(sql).trim(), "SELECT 2");
    }

    #[test]
    fn semicolon_inside_a_string_does_not_split() {
        let sql = "INSERT INTO t VALUES ('a;b');";
        let masked = mask_literals(sql);
        let stmts = split_statements(&masked);
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn start_line_counts_preceding_newlines() {
        let sql = "SELECT 1;\nSELECT 2;\nSELECT 3;";
        let masked = mask_literals(sql);
        let stmts = split_statements(&masked);
        assert_eq!(stmts[1].start_line(&masked), 2);
        assert_eq!(stmts[2].start_line(&masked), 3);
    }

    #[test]
    fn start_line_skips_a_leading_comment_line_too() {
        let sql = "-- a comment\nSELECT 1;";
        let masked = mask_literals(sql);
        let stmts = split_statements(&masked);
        assert_eq!(stmts[0].start_line(&masked), 2);
    }
}
