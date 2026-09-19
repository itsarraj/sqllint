# sqllint

Lints raw `.sql` files for real footguns — no database connection, no
`EXPLAIN`, just the text. `sqlfluff` (Python) exists but is a large,
heavily-configurable formatter/linter; this is the opposite end of the
spectrum: a small, zero-config binary that catches the handful of SQL
mistakes that actually cause incidents, not style nitpicks.

## Usage

```bash
sqllint migration.sql
sqllint migrations/*.sql
sqllint --strict migration.sql   # also fail (exit 1) on WARNING, not just ERROR
```

```
$ sqllint migration.sql
migration.sql:6: ERROR [no-where] DELETE with no WHERE clause — this affects every row in the table
migration.sql:8: WARNING [not-in-subquery] NOT IN (subquery): if the subquery returns even one NULL, the whole condition matches nothing — prefer NOT EXISTS
migration.sql:10: WARNING [implicit-cross-join] comma-separated tables in FROM with no WHERE or JOIN — if there's no join condition anywhere, this is a full cartesian product
```

## What it catches

| Rule | Severity | Why |
|---|---|---|
| `no-where` | ERROR | `UPDATE`/`DELETE` with no `WHERE` — affects every row, usually by accident |
| `select-star` | WARNING | `SELECT *` breaks silently when columns are added later |
| `null-equality` | WARNING | `= NULL` / `<> NULL` is always `UNKNOWN`, never `TRUE` — a classic silent-no-match bug |
| `not-in-subquery` | WARNING | `NOT IN (SELECT ...)` returns zero rows if the subquery yields even one `NULL` — a real, frequently-hit gotcha |
| `implicit-cross-join` | WARNING | comma-separated tables in `FROM` with no join condition anywhere — an accidental cartesian product |
| `destructive-statement` | STYLE | `DROP`/`TRUNCATE` — a heads-up, not an error |
| `trailing-whitespace` / `mixed-indentation` | STYLE | plain formatting hygiene |

Only `ERROR` fails the run by default; pass `--strict` to also fail on
`WARNING`. `STYLE` never fails the run.

String literals, quoted identifiers, and `--`/`/* */` comments are
masked out before any rule runs, so a keyword sitting inside a comment
or a string (`'run this WHERE ready'`) can never trigger a false
match — see `src/mask.rs`.

## Status: built and verified, including two real bugs caught and fixed by live-testing against a realistic file

- **26 unit tests** covering the literal/comment masking (including
  unicode content, which changes byte length per character and could
  silently break line-number alignment if masking weren't careful
  about it), statement splitting (including a semicolon *inside* a
  string not causing a false split), and every rule's true-positive
  and true-negative case.
- **Live-run against a realistic 9-statement migration file** covering
  every rule at once — and this is where two real bugs surfaced that
  the unit tests alone hadn't caught:
  - **Line numbers were silently wrong for every statement after the
    first one that followed a blank line** — `base_line` (which
    already skips leading whitespace) and a separate `line_of` helper
    (which counted newlines from the *un*trimmed statement start) were
    each independently accounting for the same leading blank lines,
    double-counting them. A statement 3 blank-line-gaps deep into a
    file reported its warnings 3 lines too late. Fixed by anchoring
    both to the same trimmed starting point. A regression test
    (`line_numbers_do_not_double_count_blank_lines_between_statements`)
    now locks this down.
  - **A leading file comment (`-- migration: cleanup` on line 1) threw
    off every subsequent line number**, because the trim step ran
    against the *original* source, where a comment isn't whitespace —
    only against the *masked* text (where comments are blanked to
    spaces) does trimming correctly skip past it. Fixed by having
    `Statement::start_line` operate on masked text throughout.
  - **`implicit-cross-join` missed the common case of aliased tables**
    (`FROM accounts a, billing b`) — the original regex expected a
    comma immediately after each table name, with no room for an
    alias in between. Fixed and covered by
    `flags_implicit_cross_join_with_table_aliases`.
  - After both fixes, the same fixture file re-run end-to-end produced
    exactly the right line number for every one of its 7 real findings,
    matched by hand against `cat -n` of the file.
- Also separately verified: a clean statement produces `no issues
  found` and exit code `0`; a warning-only file exits `0` normally and
  `1` under `--strict`; an `ERROR`-level file always exits `1`.

**Deliberate scope limits, stated plainly**: this is regex/text-based,
not a real SQL parser — it can't understand a statement's actual
structure, only recognizable patterns in it. `implicit-cross-join` in
particular is a heuristic (comma in `FROM`, no `WHERE`/`JOIN` anywhere
in the statement) that can both miss real cartesian joins hidden behind
complex `WHERE` clauses and, in principle, flag a comma-separated
`FROM` that's actually fine for reasons the regex can't see. Dialect-
specific syntax (T-SQL square-bracket identifiers, MySQL backticks) is
not handled — only ANSI-ish single/double-quote and `--`/`/* */`
comment syntax.
