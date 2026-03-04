# Tool Call Accuracy PR8 Spec

## Purpose
Add first-class read-only `glob` and `grep` builtin tools so models can search codebases without requiring `shell`.

## Scope

### In scope
- Add builtin `glob` tool
- Add builtin `grep` tool
- Add deterministic schemas/examples/error payloads
- Add execution handlers with scoped path safety
- Add tests for validation, execution, and determinism

### Out of scope
- Trust policy default changes (PR9)
- TUI approval UX changes (PR10)
- Agent mode split (`build`/`plan`) (PR11)
- Non-interactive JSON event stream enhancements (PR12)

## File-level changes

- `src/tools.rs`
  - Extend `builtin_tools_enabled(...)` to include `glob` and `grep` in read-only catalog
  - Add schema + minimal examples
  - Add arg validation for both tools
  - Add execution handlers (`run_glob`, `run_grep`) with path scoping
  - Classify side effects as `filesystem_read`
- `src/main_tests.rs`
  - Update tool catalog assertions
- `tests/artifact_golden.rs` (if tool catalog projection updates are needed)

## Tool contracts

### `glob`
- Required:
  - `pattern: string`
- Optional:
  - `path: string` (default `"."`)
  - `max_results: u32` (default `200`, min `1`, max `1000`)

### `grep`
- Required:
  - `pattern: string`
- Optional:
  - `path: string` (default `"."`)
  - `max_results: u32` (default `200`, min `1`, max `1000`)
  - `ignore_case: bool` (default `false`)

## Pattern semantics

### `glob.pattern`
- Uses `globset` pattern semantics.
- Matching is against normalized workdir-relative paths using `/` separators.
- `**` supports recursive directory matching.
- Matching is case-sensitive.

### `grep.pattern`
- Interpreted as Rust `regex` syntax (not literal by default).
- Literal search requires escaped regex text.
- `ignore_case=true` applies regex case-insensitive matching.
- Invalid regex must return deterministic `invalid_pattern` error payload.
- `grep` emits one result entry per regex match (multiple matches per line produce multiple entries).

## Response contract

Both tools return standard `openagent.tool_result.v1` envelopes.

### `glob` success content JSON
```json
{
  "matches": ["src/providers/mod.rs", "src/providers/mock.rs"],
  "match_count": 2,
  "truncated": false,
  "max_results": 200
}
```

### `grep` success content JSON
```json
{
  "matches": [
    {"path":"README.md","line":2,"column":1,"text":"TODO: validate fallback path"}
  ],
  "match_count": 1,
  "truncated": false,
  "max_results": 200,
  "skipped_binary_or_non_utf8_files": 0
}
```

If `max_results` cap is hit:
- `truncated=true`
- only the first `max_results` matches are returned after deterministic sorting.

## Sorting and truncation semantics

- `glob.matches` are sorted lexicographically by normalized relative path.
- `grep.matches` are sorted by:
  1. normalized relative path (lexicographic)
  2. line number (ascending)
  3. column number (ascending)
  4. text (lexicographic tie-breaker)
- Truncation is applied after sorting; returned items are the first `max_results` entries.

## Path and position contracts

- All returned `path` values are normalized, workdir-relative paths (never absolute).
- Path normalization uses forward slashes in envelope content for cross-platform stability.
- For `grep.matches`:
  - `line` is 1-based line index.
  - `column` is 1-based UTF-8 byte offset for match start within the line.
  - `text` is line content with trailing `\n` removed; for CRLF input, trailing `\r` is also removed.

## Path safety and symlink scoping

- `path` must be workdir-scoped via existing `resolve_path_scoped` rules:
  - reject absolute paths
  - reject `..` traversal escapes
- Symlink traversal must not escape workdir:
  - canonicalized target must remain under canonicalized workdir
  - out-of-root symlink targets are skipped with deterministic warning metadata and never followed

## File inclusion/exclusion rules

- Search tools exclude any path containing a segment exactly equal to `.git`.
- No additional ignore-file behavior is added in PR8 (future enhancement).

## `grep` file handling behavior

- `grep` scans text files only.
- Binary/non-text detection is deterministic:
  - treat file as non-text if UTF-8 decode fails or file contains `\0`.
- Binary or non-UTF8 files are skipped (not fatal).
- Skipped count is reported in `skipped_binary_or_non_utf8_files`.

## Warning metadata contract

When symlink-out-of-root skips occur, include `meta.warnings` entries:

```json
{
  "code": "symlink_out_of_scope_skipped",
  "path": "relative/path/of/symlink",
  "target": "OUT_OF_SCOPE",
  "reason": "target escapes workdir"
}
```

- `code`, `path`, `target`, and `reason` are required.
- `target` is sentinel literal `OUT_OF_SCOPE` (never canonical absolute host path).
- Warning order is deterministic (sorted by `path`, then `code`).
- Warning list is bounded:
  - `meta.warnings_max = 50`
  - `meta.warnings_truncated: bool`
  - truncation applies after sort.

## Error contract

Argument/path/pattern failures must use deterministic structured errors with stable `code` values:
- `invalid_arguments`
- `invalid_pattern`
- `path_out_of_scope`
- `io_error`

Error payload must include:
- `code`
- `message`
- `tool`
- `minimal_example` (where already supported by existing tool error style)

## Determinism requirements

- Stable defaults (`path="."`, `max_results=200`, `ignore_case=false`)
- Stable ordering and truncation semantics
- Stable path normalization and relativity guarantees
- Stable non-text skip behavior + counted metadata
- Stable warning ordering/bounds/truncation metadata
- Structured deterministic error payloads

## Test plan

### Unit tests (`src/tools.rs`)
- `builtin_readonly_catalog_includes_glob_and_grep`
- `glob_args_validation_rejects_missing_pattern`
- `grep_args_validation_rejects_missing_pattern`
- `glob_uses_default_path_and_max_results`
- `grep_uses_default_path_ignore_case_and_max_results`
- `glob_rejects_absolute_path`
- `glob_rejects_path_traversal`
- `grep_rejects_absolute_path`
- `grep_rejects_path_traversal`
- `grep_rejects_invalid_regex_pattern`
- `glob_results_are_sorted_deterministically`
- `grep_results_are_sorted_deterministically`
- `glob_truncates_at_max_results_and_sets_truncated`
- `grep_truncates_at_max_results_and_sets_truncated`
- `grep_skips_non_utf8_or_binary_files_and_reports_count`
- `glob_does_not_follow_symlink_outside_workdir`
- `grep_does_not_follow_symlink_outside_workdir`
- `glob_returns_relative_normalized_paths_only`
- `grep_returns_relative_normalized_paths_only`
- `grep_line_is_1_based_and_column_is_1_based_utf8_byte_offset`
- `grep_emits_multiple_entries_for_multiple_matches_on_same_line`
- `grep_line_text_is_newline_trimmed_and_crlf_normalized`
- `glob_excludes_dot_git_directory`
- `grep_excludes_dot_git_directory`
- `symlink_skip_warning_metadata_is_present_and_deterministic`
- `symlink_skip_warning_target_uses_out_of_scope_sentinel`
- `warning_list_is_bounded_and_reports_warnings_truncated`

### Integration/golden
- update tool-catalog expected set where asserted
- ensure artifact schema remains backward compatible

## Verification commands
```bash
cargo test tools::
cargo test main_tests::
cargo test artifact_golden -- --nocapture
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Implementation checklist
- [ ] Add `glob`/`grep` to builtin tool catalog
- [ ] Add schemas and minimal examples
- [ ] Add arg validation and deterministic error payloads
- [ ] Enforce `max_results` bounds/defaults (1..=1000, default 200)
- [ ] Implement scoped execution handlers
- [ ] Enforce symlink escape prevention under workdir scoping
- [ ] Implement deterministic sorting + truncation semantics
- [ ] Implement non-UTF8/binary skip behavior for `grep`
- [ ] Implement warning metadata contract + warning bounds
- [ ] Mark side effects as `filesystem_read`
- [ ] Add/adjust unit and integration tests
- [ ] Run full verification command set

## Exit criteria
- `glob` and `grep` are available without `--allow-shell`
- searching is possible in read-only mode
- tests and lint pass
