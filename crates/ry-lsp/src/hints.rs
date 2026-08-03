//! Inlay-hint, completion, and signature-help helpers.

use ry_checker::Scope;
use ry_core::{Expr, Mode, SourceFile, Stmt};
use tower_lsp::lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, InlayHint, InlayHintKind,
    InlayHintLabel, Position,
};

use crate::util::{byte_offset_to_position, position_to_byte_offset};

/// Collect `InlayHint`s for every assignment whose target is a bare
/// identifier with a known (non-opaque) inferred type. The hint is
/// placed at the end of the identifier name (so the editor renders the
/// ghost text right after the variable, before the `<-`), and its
/// label is the inferred type rendered via `RType`'s `Display` impl
/// (e.g. `: integer<len=1>`).
///
/// The walk recurses into `Stmt::FunctionDef` bodies so that local
/// bindings inside named functions are annotated too (the top-level
/// scope may or may not track them; if it doesn't, the lookup simply
/// yields `None` and no hint is emitted, which is the right call for
/// v1).
///
/// Opaque (`Mode::Opaque`) types are deliberately skipped: they
/// represent "we don't know" and would only clutter the editor with
/// unhelpful `: opaque<len=?>?NA?` annotations. This mirrors how the
/// `document_symbol` detail path behaves implicitly (it surfaces
/// whatever the scope has, but for opaque the Display string is
/// noisy). For inlay hints, skipping is the better UX.
pub(super) fn collect_inlay_hints(file: &SourceFile, scope: &Scope, text: &str) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for stmt in &file.stmts {
        collect_inlay_hints_from_stmt(stmt, scope, text, &mut hints);
    }
    hints
}

/// Walk a single statement, appending any inlay hints it contributes
/// to `hints`. Assignments to a bare identifier become hints (when
/// the scope has a non-opaque type for the name); function-definition
/// statements are recursed into so their body bindings are annotated.
fn collect_inlay_hints_from_stmt(
    stmt: &Stmt,
    scope: &Scope,
    text: &str,
    hints: &mut Vec<InlayHint>,
) {
    match stmt {
        // Destructure the target directly so clippy's `collapsible_match`
        // lint stays quiet: only bare-identifier targets become hints.
        // Complex targets (`df$col <- 1`, `x[1] <- 2`) fall through to
        // the second `Stmt::Assign` arm below and contribute nothing.
        Stmt::Assign {
            target: Expr::Ident { name, span },
            ..
        } => {
            if let Some(t) = scope.get(name) {
                // Skip opaque types: they're not useful to the
                // user (they represent "we don't know"). Showing
                // `: opaque<len=?>?NA?` next to every unknown
                // binding would just be visual noise.
                if matches!(t.mode, ry_core::types::Mode::Opaque) {
                    return;
                }
                // Place the hint right after the identifier name.
                // `span.start + name.len()` lands on the first
                // character past the identifier (the identifier's
                // byte end); `byte_offset_to_position` converts it to
                // a UTF-16 LSP `Position`, so non-ASCII identifiers
                // are handled correctly too.
                let pos = byte_offset_to_position(text, span.start + name.len());
                hints.push(InlayHint {
                    position: pos,
                    label: InlayHintLabel::String(format!(": {}", t)),
                    kind: Some(InlayHintKind::TYPE),
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    text_edits: None,
                    data: None,
                });
            }
        }
        // Non-identifier assignment targets (e.g. `x[1] <- 2`,
        // `df$col <- value`) don't introduce a new name in the
        // scope, so they contribute no hints.
        Stmt::Assign { .. } => {}
        // Recurse into named function bodies so nested bindings are
        // annotated too. `Stmt::FunctionDef` with `name: Some(..)` is
        // not currently emitted by the parser (named functions come
        // through as `Assign` + `Expr::Function`), but we handle it
        // for completeness / future grammar changes.
        Stmt::FunctionDef { body, .. } => {
            for s in body {
                collect_inlay_hints_from_stmt(s, scope, text, hints);
            }
        }
        // Other statement forms (bare expressions, control flow,
        // returns) do not introduce named top-level bindings, so they
        // contribute no hints. We deliberately do NOT recurse into
        // `if`/`for`/`while` bodies here (unlike `collect_symbols`)
        // because the top-level scope only tracks the file's top
        // scope; bindings introduced inside control-flow blocks may
        // not be present in `scope`, and emitting a hint for a name
        // the scope doesn't know would be wrong.
        _ => {}
    }
}

/// Collect completion items for a given cursor position and trigger
/// context. The decision tree mirrors what R users expect from an
/// autocomplete popup:
///
///   * If the user just typed `$` after an identifier whose scope
///     type carries a `ColumnSchema` (e.g. a `list(a = 1, b = 2)`
///     literal or a `data.frame(...)` call), return ONLY the column
///     names as `FIELD` items. Dumping the rest of the scope here
///     would be noise: the user is clearly asking for a column.
///   * Otherwise (manual invocation, identifier character, `:`, etc.)
///     return the in-scope bindings (`VARIABLE` / `FUNCTION`) plus a
///     curated list of common base-R keywords and functions. This
///     gives a focused, predictable popup instead of a giant dump.
///
/// The list is sorted alphabetically by `label` and de-duplicated so
/// the same name never appears twice (e.g. when a user-defined `c`
/// would otherwise collide with the curated `c`).
pub(super) fn collect_completions(
    text: &str,
    position: Position,
    context: &Option<CompletionContext>,
    scope: &Scope,
) -> Vec<CompletionItem> {
    let trigger = context
        .as_ref()
        .and_then(|c| c.trigger_character.as_deref());

    if trigger == Some("$") {
        // `$`-triggered completion: offer only column names from the
        // variable before the `$` on the current line. If the
        // variable is unknown or carries no schema, return an empty
        // list (no completions) rather than falling through to the
        // generic list, so the editor popup stays focused.
        if let Some(line) = text.lines().nth(position.line as usize) {
            // `position.character` is a UTF-16 offset, not a byte
            // index; resolve it to a byte offset before slicing so a
            // line with a non-ASCII character before the cursor cannot
            // panic (or truncate) the slice.
            let until = position_to_byte_offset(text, position.line, position.character)
                .unwrap_or(text.len());
            let before_cursor = &line[..until.min(line.len())];
            // Strip the trailing `$` (and any whitespace between the
            // identifier and it) so `extract_last_identifier` lands
            // on the variable name.
            let trimmed = before_cursor.trim_end().trim_end_matches('$');
            if let Some(var_name) = extract_last_identifier(trimmed) {
                if let Some(t) = scope.get(&var_name) {
                    if let Some(schema) = &t.columns {
                        let mut items: Vec<CompletionItem> = schema
                            .columns
                            .iter()
                            .map(|(col_name, col_type)| CompletionItem {
                                label: col_name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(format!("{}", col_type)),
                                ..Default::default()
                            })
                            .collect();
                        items.sort_by(|a, b| a.label.cmp(&b.label));
                        return items;
                    }
                }
            }
        }
        // No schema (or no variable) before the `$`: nothing useful
        // to offer. Returning empty lets the editor close the popup
        // instead of showing irrelevant completions.
        return Vec::new();
    }

    // Generic completion: variables in scope + common keywords /
    // functions. We surface every checked binding (so locally defined
    // variables and functions complete) and then layer in the small
    // curated list of base-R names. The curated list is intentionally
    // short: the task explicitly calls for a focused popup.
    let mut items: Vec<CompletionItem> = scope
        .bindings
        .iter()
        .map(|(name, t)| CompletionItem {
            label: name.clone(),
            kind: Some(if matches!(t.mode, Mode::Function) {
                CompletionItemKind::FUNCTION
            } else {
                CompletionItemKind::VARIABLE
            }),
            detail: Some(format!("{}", t)),
            ..Default::default()
        })
        .collect();
    items.extend(common_r_completions());

    // Sort alphabetically by label, then drop duplicates (a user
    // `c <- ...` binding would otherwise collide with the curated
    // `c`). `dedup_by` after `sort_by` collapses only adjacent
    // equal-label pairs, so the sort must use the same key.
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items
}

/// Build a small, curated list of common base-R keywords and
/// functions. Kept short on purpose: the task calls for a focused
/// popup, and dumping the typeshed's full function table (thousands of
/// entries) would bury the common names under noise.
///
/// Keyword entries are pure R syntax, so they cannot come from the
/// typeshed and stay curated. The function entries keep a curated name
/// plus a one-line human hint (the typeshed has no descriptions), but
/// every name is validated against the base typeshed: a function must
/// appear in its `functions` map or `ambient_functions` globals or it
/// is dropped, so the popup cannot offer a name the checker does not
/// know. We use the full `CompletionItemKind::X` form (rather than a
/// `use` alias) because `CompletionItemKind` is a tuple struct with
/// associated constants, not an enum, so a glob import is not allowed.
pub(super) fn common_r_completions() -> Vec<CompletionItem> {
    const KEYWORD_ENTRIES: &[(&str, &str)] = &[
        // Keywords / control flow.
        ("if", "conditional"),
        ("else", "conditional alternative"),
        ("for", "for loop"),
        ("while", "while loop"),
        ("repeat", "repeat loop"),
        ("function", "function definition"),
        ("return", "return from function"),
        ("break", "break out of loop"),
        ("next", "skip to next iteration"),
    ];
    // (name, detail). The detail is a one-line human hint so the
    // popup shows something useful next to each entry.
    const FUNCTION_ENTRIES: &[(&str, &str)] = &[
        ("c", "combine values into a vector"),
        ("list", "create a list"),
        ("data.frame", "create a data frame"),
        ("matrix", "create a matrix"),
        ("vector", "create a vector"),
        ("length", "length of an object"),
        ("names", "names of an object"),
        ("mean", "arithmetic mean"),
        ("sum", "sum of elements"),
        ("min", "minimum"),
        ("max", "maximum"),
        ("print", "print an object"),
        ("str", "display the structure of an object"),
        ("library", "load an attached package"),
        ("require", "load an attached package"),
        ("sapply", "apply a function over a list or vector"),
        ("lapply", "apply a function over a list"),
        ("mapply", "apply a function over multiple arguments"),
        ("which", "indices of TRUE values"),
        ("is.na", "detect missing values"),
        ("as.integer", "coerce to integer"),
        ("as.numeric", "coerce to numeric"),
        ("as.character", "coerce to character"),
        ("as.logical", "coerce to logical"),
    ];
    let base = ry_typeshed::load_base_cached().ok();
    let mut items: Vec<CompletionItem> = KEYWORD_ENTRIES
        .iter()
        .map(|(name, detail)| CompletionItem {
            label: (*name).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some((*detail).to_string()),
            ..Default::default()
        })
        .collect();
    items.extend(
        FUNCTION_ENTRIES
            .iter()
            .filter(|(name, _)| {
                base.is_none_or(|typeshed| {
                    typeshed.functions.contains_key(*name)
                        || typeshed
                            .globals
                            .ambient_functions
                            .iter()
                            .any(|n| n == *name)
                })
            })
            .map(|(name, detail)| CompletionItem {
                label: (*name).to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some((*detail).to_string()),
                ..Default::default()
            }),
    );
    items
}

/// Extract the identifier at the end of `s`, scanning backwards. An
/// "identifier character" follows R's rules: ASCII alphanumeric, `_`,
/// or `.`. Returns `None` when `s` does not end with an identifier
/// character (e.g. `s == ""`, `s == "()"`, or `s == "$"`).
///
/// This is used by `$`-triggered completion to recover the variable
/// name preceding the `$` (e.g. `mtcars$` -> `mtcars`). It is a
/// simple character scan, not a parser query, which is enough for
/// the common single-line case `var$`.
pub(super) fn extract_last_identifier(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut end = chars.len();
    while end > 0
        && (chars[end - 1].is_alphanumeric() || chars[end - 1] == '_' || chars[end - 1] == '.')
    {
        end -= 1;
    }
    if end < chars.len() {
        Some(chars[end..].iter().collect())
    } else {
        None
    }
}

/// Find the enclosing function call for a cursor at `(line, col)` in
/// `text`. Returns `(function_name, active_param_index)` where
/// `function_name` is the identifier immediately before the nearest
/// unmatched `(` to the left of the cursor, and `active_param_index`
/// is the number of commas at depth 0 between the `(` and the cursor.
///
/// The scan is confined to the current line (matching the common case
/// where the user is mid-call on a single line). Returns `None` when:
///   * the line doesn't exist;
///   * there is no unmatched `(` before the cursor (cursor not in a
///     call);
///   * the text immediately before the `(` is not an identifier
///     (e.g. the cursor sits inside `1 + (2 *` rather than a function
///     call).
pub(super) fn find_enclosing_call(text: &str, line: usize, col: usize) -> Option<(String, usize)> {
    let line_str = text.lines().nth(line)?;
    // `col` is a UTF-16 code-unit column from the LSP request.
    // Resolve it to a byte offset before slicing, so a line with a
    // non-ASCII character before the cursor cannot panic the slice.
    // A cursor past the last character (a common transient state
    // right after typing `(`) is clamped to the line end.
    let until = position_to_byte_offset(text, line as u32, col as u32).unwrap_or(text.len());
    let before_cursor = &line_str[..until.min(line_str.len())];

    // Walk backward to find the last unmatched `(`. We track depth so
    // a `(` belonging to a nested call (e.g. the inner `(` in
    // `f(g(`) is skipped in favor of the outermost enclosing one.
    let mut depth = 0;
    let mut paren_pos = None;
    for (i, ch) in before_cursor.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let paren_pos = paren_pos?;

    // The function name is the identifier ending right at the `(`.
    // `extract_last_identifier` already scans backward for an R-style
    // identifier, which is exactly what we need.
    let before_paren = &before_cursor[..paren_pos];
    let func_name = extract_last_identifier(before_paren)?;

    // Count commas between `(` and the cursor to determine which
    // parameter the user is currently editing. We only count commas at
    // depth 0 (commas inside nested calls belong to the inner call's
    // argument list, not this one). Strings are not tracked here, so
    // a comma inside a string literal would be miscounted; that's an
    // acceptable v1 approximation for the common case.
    let args_str = &before_cursor[paren_pos + 1..];
    let mut local_depth = 0;
    let mut active_param = 0;
    for ch in args_str.chars() {
        match ch {
            '(' | '[' | '{' => local_depth += 1,
            ')' | ']' | '}' => {
                if local_depth > 0 {
                    local_depth -= 1;
                }
            }
            ',' if local_depth == 0 => active_param += 1,
            _ => {}
        }
    }

    Some((func_name, active_param))
}

/// Look up the formal parameter names of a base-R function for
/// signature help. Returns `None` for functions outside the base
/// typeshed (user-defined functions are out of scope; the checker's
/// FnTable isn't reachable from the LSP crate).
///
/// The embedded base typeshed is the single source of truth for
/// parameter names: there is no parallel hand-maintained table here,
/// so the LSP cannot drift from what the checker resolves.
pub(super) fn get_signature(name: &str) -> Option<Vec<String>> {
    let base = ry_typeshed::load_base_cached().ok()?;
    let signature = base.functions.get(name)?;
    Some(signature.param_names().map(str::to_owned).collect())
}
