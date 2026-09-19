/// Replaces the *content* of string/identifier literals and comments
/// with spaces (newlines preserved), byte-for-byte the same length as
/// the input. Every rule matches against this masked text instead of
/// the raw file, so a keyword like `WHERE` sitting inside a comment or
/// a string literal (`'the WHERE clause is optional here'`) can never
/// produce a false match — and because masking preserves both length
/// and line breaks, a byte offset found in the masked text is exactly
/// the same offset in the original file, so line numbers stay correct
/// without a second pass.
pub fn mask_literals(sql: &str) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum State {
        Normal,
        Single,
        Double,
        LineComment,
        BlockComment,
    }

    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut state = State::Normal;

    let mask_char = |out: &mut String, c: char| {
        if c == '\n' {
            out.push('\n');
        } else {
            out.push_str(&" ".repeat(c.len_utf8()));
        }
    };

    while let Some(c) = chars.next() {
        match state {
            State::Normal => match c {
                '\'' => {
                    state = State::Single;
                    mask_char(&mut out, c);
                }
                '"' => {
                    state = State::Double;
                    mask_char(&mut out, c);
                }
                '-' if chars.peek() == Some(&'-') => {
                    mask_char(&mut out, c);
                    mask_char(&mut out, chars.next().unwrap());
                    state = State::LineComment;
                }
                '/' if chars.peek() == Some(&'*') => {
                    mask_char(&mut out, c);
                    mask_char(&mut out, chars.next().unwrap());
                    state = State::BlockComment;
                }
                _ => out.push(c),
            },
            State::Single => {
                if c == '\'' && chars.peek() == Some(&'\'') {
                    // Escaped '' inside a string — stays masked, stays inside the string.
                    mask_char(&mut out, c);
                    mask_char(&mut out, chars.next().unwrap());
                } else if c == '\'' {
                    state = State::Normal;
                    mask_char(&mut out, c);
                } else {
                    mask_char(&mut out, c);
                }
            }
            State::Double => {
                if c == '"' {
                    state = State::Normal;
                }
                mask_char(&mut out, c);
            }
            State::LineComment => {
                if c == '\n' {
                    state = State::Normal;
                }
                mask_char(&mut out, c);
            }
            State::BlockComment => {
                if c == '*' && chars.peek() == Some(&'/') {
                    mask_char(&mut out, c);
                    mask_char(&mut out, chars.next().unwrap());
                    state = State::Normal;
                } else {
                    mask_char(&mut out, c);
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_length_and_line_count() {
        let sql = "SELECT 'a\nb' FROM t; -- WHERE trap\nDELETE FROM t;";
        let masked = mask_literals(sql);
        assert_eq!(sql.len(), masked.len());
        assert_eq!(sql.matches('\n').count(), masked.matches('\n').count());
    }

    #[test]
    fn keyword_inside_string_literal_is_masked_out() {
        let sql = "UPDATE t SET note = 'run this WHERE ready'";
        let masked = mask_literals(sql);
        assert!(!masked.to_uppercase().contains("WHERE"));
    }

    #[test]
    fn keyword_inside_line_comment_is_masked_out() {
        let sql = "DELETE FROM t -- WHERE id = 1\n";
        let masked = mask_literals(sql);
        assert!(!masked.to_uppercase().contains("WHERE"));
    }

    #[test]
    fn keyword_inside_block_comment_is_masked_out() {
        let sql = "DELETE FROM t /* WHERE id = 1 */";
        let masked = mask_literals(sql);
        assert!(!masked.to_uppercase().contains("WHERE"));
    }

    #[test]
    fn real_where_clause_survives_masking() {
        let sql = "DELETE FROM t WHERE id = 1";
        let masked = mask_literals(sql);
        assert!(masked.to_uppercase().contains("WHERE"));
    }

    #[test]
    fn escaped_quote_inside_string_does_not_end_the_string_early() {
        let sql = "SELECT 'it''s WHERE fine' FROM t";
        let masked = mask_literals(sql);
        assert!(!masked.to_uppercase().contains("WHERE"));
    }

    #[test]
    fn unicode_content_does_not_break_byte_alignment() {
        let sql = "SELECT 'héllo wörld' FROM t WHERE x = 1";
        let masked = mask_literals(sql);
        assert_eq!(sql.len(), masked.len());
        assert!(masked.to_uppercase().contains("WHERE"));
    }
}
