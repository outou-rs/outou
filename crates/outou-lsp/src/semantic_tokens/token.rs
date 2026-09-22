//! Pure token-list transforms: delta <-> absolute encoding, splitting a
//! (possibly multi-line) span into one single-line token per line, and
//! merging rust-analyzer-derived tokens with Outou's own overlay tokens.
//! None of this needs a live rust-analyzer; see this module's tests.

use outou_sourcemap::{LineIndex, Position, PositionRange, Span};

/// One semantic token in absolute (not delta-encoded) coordinates, always
/// confined to a single line (the LSP semantic tokens contract forbids a
/// token spanning a line break).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsoluteToken {
    pub line: u32,
    /// UTF-16 code unit offset within the line.
    pub start: u32,
    /// Length in UTF-16 code units.
    pub length: u32,
    /// Index into the legend's `token_types` in effect for this response.
    pub token_type: u32,
    /// Bitset of modifier indices into the legend's `token_modifiers`.
    pub modifiers: u32,
}

/// Decodes the LSP delta-encoded `SemanticToken` list into absolute
/// coordinates. Per the LSP spec, `delta_line`/`delta_start` are relative
/// to the *previous* token's start; `delta_start` resets to an absolute
/// column only when `delta_line` is nonzero.
///
/// rust-analyzer-supplied deltas are untrusted input (issue #14 review,
/// SHOULD-LAND-8): accumulation uses `saturating_add` rather than plain
/// `+`, so a pathological or malformed delta (or, in principle, a
/// currently-inconceivable but not impossible run of tokens whose deltas
/// sum past `u32::MAX`) saturates at `u32::MAX` instead of panicking on
/// overflow — a panic here would take down the single shared dispatch
/// loop, not just this one request. Saturating also keeps the output
/// non-decreasing in `line` (every subsequent `line` is `line +
/// non-negative delta`, so it can only saturate at the same ceiling,
/// never wrap back below a previous value), preserving the sortedness
/// [`merge`]/[`encode`] rely on.
pub fn decode(tokens: &[lsp_types::SemanticToken]) -> Vec<AbsoluteToken> {
    let mut line = 0u32;
    let mut start = 0u32;
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        line = line.saturating_add(token.delta_line);
        start = if token.delta_line == 0 {
            start.saturating_add(token.delta_start)
        } else {
            token.delta_start
        };
        out.push(AbsoluteToken {
            line,
            start,
            length: token.length,
            token_type: token.token_type,
            modifiers: token.token_modifiers_bitset,
        });
    }
    out
}

/// Encodes absolute tokens back into the LSP delta form. Sorts by
/// `(line, start)` first (the LSP spec requires tokens in that order; delta
/// encoding a list that is not already sorted would produce nonsensical,
/// possibly negative, deltas), so callers do not have to sort separately.
pub fn encode(mut tokens: Vec<AbsoluteToken>) -> Vec<lsp_types::SemanticToken> {
    tokens.sort_by_key(|t| (t.line, t.start));
    let mut out = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for (i, token) in tokens.iter().enumerate() {
        let delta_line = token.line - prev_line;
        let delta_start = if i == 0 || delta_line != 0 {
            token.start
        } else {
            token.start - prev_start
        };
        out.push(lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers,
        });
        prev_line = token.line;
        prev_start = token.start;
    }
    out
}

/// Converts one (possibly multi-line) byte span into one [`AbsoluteToken`]
/// per line it covers, per the LSP rule that a single token cannot cross a
/// line break. A single-line span produces exactly one token; a span
/// crossing `n` line breaks produces `n + 1` tokens, one per line, each
/// clipped to that line's own content.
///
/// A zero-length result (a span whose mapped range is empty) is dropped:
/// there is nothing to highlight.
pub fn span_to_line_tokens(
    line_index: &LineIndex,
    span: Span,
    token_type: u32,
    modifiers: u32,
) -> Vec<AbsoluteToken> {
    let range = line_index.span_to_range(span);
    if range.start.line == range.end.line {
        return single_line_token(range, token_type, modifiers)
            .into_iter()
            .collect();
    }
    let mut out = Vec::new();
    for line in range.start.line..=range.end.line {
        let start_char = if line == range.start.line {
            range.start.character
        } else {
            0
        };
        let end_char = if line == range.end.line {
            range.end.character
        } else {
            line_end_character(line_index, line)
        };
        if end_char > start_char {
            out.push(AbsoluteToken {
                line,
                start: start_char,
                length: end_char - start_char,
                token_type,
                modifiers,
            });
        }
    }
    out
}

fn single_line_token(
    range: PositionRange,
    token_type: u32,
    modifiers: u32,
) -> Option<AbsoluteToken> {
    if range.end.character <= range.start.character {
        return None;
    }
    Some(AbsoluteToken {
        line: range.start.line,
        start: range.start.character,
        length: range.end.character - range.start.character,
        token_type,
        modifiers,
    })
}

/// The UTF-16 length of `line`'s own content (before its line terminator),
/// found through the public `LineIndex` API alone (no private accessor
/// needed): asking for a character index past a line's end clamps to the
/// content end (`LineIndex::position_to_offset`'s own documented
/// behavior), so round-tripping that clamped offset back through
/// `offset_to_position` recovers the true content length.
fn line_end_character(line_index: &LineIndex, line: u32) -> u32 {
    let offset = line_index.position_to_offset(Position::new(line, u32::MAX));
    line_index.offset_to_position(offset).character
}

/// Whether two same-line tokens overlap (share any UTF-16 column).
fn overlaps(a: AbsoluteToken, b: AbsoluteToken) -> bool {
    a.line == b.line
        && a.start < b.start.saturating_add(b.length)
        && b.start < a.start.saturating_add(a.length)
}

/// Merges rust-analyzer-derived tokens with Outou's own AST-derived
/// overlay tokens: Outou's own tokens always win a JSX position, since
/// they carry vocabulary rust-analyzer cannot know (component vs. HTML
/// element, event vs. plain attribute) — any `ra_tokens` entry that
/// overlaps an `outou_tokens` entry is dropped rather than kept alongside
/// it, per `docs/adr/0012...md`'s overlay precedence decision. The result
/// is sorted by `(line, start)`, satisfying the "never overlapping or
/// out-of-order" requirement together with the per-line disjointness
/// [`span_to_line_tokens`] and the AST walk already guarantee for each
/// input list on its own.
pub fn merge(
    ra_tokens: Vec<AbsoluteToken>,
    outou_tokens: Vec<AbsoluteToken>,
) -> Vec<AbsoluteToken> {
    let kept_ra: Vec<AbsoluteToken> = ra_tokens
        .into_iter()
        .filter(|ra| !outou_tokens.iter().any(|outou| overlaps(*ra, *outou)))
        .collect();
    let mut merged: Vec<AbsoluteToken> = kept_ra.into_iter().chain(outou_tokens).collect();
    merged.sort_by_key(|t| (t.line, t.start));
    drop_overlaps_stable(merged)
}

/// Final, unconditional non-overlap pass (issue #14 review, BLOCKING-2):
/// whatever the source-map narrowing upstream produced, the LSP
/// semantic-tokens invariant ("no two tokens on the same line may
/// overlap, and they must be in order") must hold regardless of how
/// coarse or malformed a mapping might turn out to be. First-wins, over
/// `sorted` (already ordered by `(line, start)`): the earlier token is
/// kept and every later one overlapping it on the same line is dropped.
///
/// This never costs an Outou-native token its overlay priority: the
/// caller ([`merge`]) has already dropped every rust-analyzer token that
/// overlapped an Outou one *before* sorting, so by the time this pass
/// runs, a surviving overlap can only be rust-analyzer-vs-rust-analyzer
/// (two of its own tokens whose mapped `.rsx` ranges turned out to
/// coincide or cross) or, defensively, Outou-vs-Outou, which the AST walk
/// should never produce overlapping in the first place.
///
/// Correct with only a look-back at the single most recently kept token
/// because the input is sorted by non-decreasing `start`: once a
/// candidate's start has advanced past every earlier kept token's start,
/// it can only still overlap the most recently kept one (whichever kept
/// token has the largest end so far) — this is the standard
/// merge-intervals scan.
fn drop_overlaps_stable(sorted: Vec<AbsoluteToken>) -> Vec<AbsoluteToken> {
    let mut kept: Vec<AbsoluteToken> = Vec::with_capacity(sorted.len());
    for token in sorted {
        let overlaps_kept = kept.last().is_some_and(|&last| overlaps(last, token));
        if !overlaps_kept {
            kept.push(token);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(
        delta_line: u32,
        delta_start: u32,
        length: u32,
        token_type: u32,
    ) -> lsp_types::SemanticToken {
        lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn decode_accumulates_line_and_resets_start_on_a_new_line() {
        let tokens = vec![token(1, 4, 3, 0), token(0, 5, 2, 1), token(2, 1, 1, 2)];
        let decoded = decode(&tokens);
        assert_eq!(
            decoded[0],
            AbsoluteToken {
                line: 1,
                start: 4,
                length: 3,
                token_type: 0,
                modifiers: 0
            }
        );
        assert_eq!(
            decoded[1],
            AbsoluteToken {
                line: 1,
                start: 9,
                length: 2,
                token_type: 1,
                modifiers: 0
            }
        );
        assert_eq!(
            decoded[2],
            AbsoluteToken {
                line: 3,
                start: 1,
                length: 1,
                token_type: 2,
                modifiers: 0
            }
        );
    }

    /// Issue #14 review (SHOULD-LAND-8): rust-analyzer-supplied token
    /// coordinates are untrusted input; a `deltaLine`/`deltaStart` that
    /// would overflow `u32` accumulation must saturate, never panic (a
    /// panic in a shared dispatch loop takes the whole server down).
    #[test]
    fn decode_saturates_rather_than_overflows_on_pathological_deltas() {
        let tokens = vec![token(u32::MAX, 0, 1, 0), token(1, 0, 1, 0)];
        let decoded = decode(&tokens); // must not panic
        assert_eq!(decoded[0].line, u32::MAX);
        assert_eq!(decoded[1].line, u32::MAX); // saturated, not wrapped
                                               // Still ordered (non-decreasing line), matching the invariant
                                               // `merge`/`encode` rely on.
        assert!(decoded[0].line <= decoded[1].line);
    }

    #[test]
    fn encode_is_the_inverse_of_decode() {
        let original = vec![token(1, 4, 3, 0), token(0, 5, 2, 1), token(2, 1, 1, 2)];
        let round_tripped = encode(decode(&original));
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn encode_sorts_unordered_input_before_delta_encoding() {
        let tokens = vec![
            AbsoluteToken {
                line: 3,
                start: 1,
                length: 1,
                token_type: 0,
                modifiers: 0,
            },
            AbsoluteToken {
                line: 1,
                start: 4,
                length: 3,
                token_type: 0,
                modifiers: 0,
            },
        ];
        let encoded = encode(tokens);
        // Sorted order first: line 1 at delta_line 1 from origin, then line
        // 3 at delta_line 2 from line 1.
        assert_eq!(encoded[0].delta_line, 1);
        assert_eq!(encoded[0].delta_start, 4);
        assert_eq!(encoded[1].delta_line, 2);
        assert_eq!(encoded[1].delta_start, 1);
    }

    #[test]
    fn span_to_line_tokens_produces_one_token_for_a_single_line_span() {
        let line_index = LineIndex::new("let user = load_user();\n");
        let span = Span::new(4, 8); // "user"
        let tokens = span_to_line_tokens(&line_index, span, 7, 0);
        assert_eq!(
            tokens,
            vec![AbsoluteToken {
                line: 0,
                start: 4,
                length: 4,
                token_type: 7,
                modifiers: 0
            }]
        );
    }

    #[test]
    fn span_to_line_tokens_splits_a_multi_line_span_per_line() {
        // "Hello\nWorld" spans two lines; the span covers bytes 0..11.
        let line_index = LineIndex::new("Hello\nWorld\n");
        let span = Span::new(0, 11);
        let tokens = span_to_line_tokens(&line_index, span, 18, 0);
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[0],
            AbsoluteToken {
                line: 0,
                start: 0,
                length: 5,
                token_type: 18,
                modifiers: 0
            }
        );
        assert_eq!(
            tokens[1],
            AbsoluteToken {
                line: 1,
                start: 0,
                length: 5,
                token_type: 18,
                modifiers: 0
            }
        );
    }

    #[test]
    fn span_to_line_tokens_drops_a_zero_width_span() {
        let line_index = LineIndex::new("abc\n");
        let tokens = span_to_line_tokens(&line_index, Span::new(1, 1), 0, 0);
        assert!(tokens.is_empty());
    }

    #[test]
    fn merge_drops_a_rust_analyzer_token_that_overlaps_an_outou_token() {
        let ra = vec![AbsoluteToken {
            line: 0,
            start: 1,
            length: 4,
            token_type: 12,
            modifiers: 0,
        }];
        let outou = vec![AbsoluteToken {
            line: 0,
            start: 1,
            length: 4,
            token_type: 2,
            modifiers: 0,
        }];
        let merged = merge(ra, outou.clone());
        assert_eq!(merged, outou);
    }

    #[test]
    fn merge_keeps_non_overlapping_tokens_from_both_sides_sorted() {
        let ra = vec![AbsoluteToken {
            line: 0,
            start: 10,
            length: 3,
            token_type: 12,
            modifiers: 0,
        }];
        let outou = vec![AbsoluteToken {
            line: 0,
            start: 1,
            length: 4,
            token_type: 2,
            modifiers: 0,
        }];
        let merged = merge(ra, outou);
        assert_eq!(merged[0].start, 1);
        assert_eq!(merged[1].start, 10);
    }
}
