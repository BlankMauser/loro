# Draft PR: `LoroText::history_order`, a read-only Fugue order with deleted characters

Status: draft description for an upstream PR to loro-dev/loro. Not opened yet;
the fork branch is `BlankMauser/loro` `claude/text-order`, based on the
`loro-v1.16.2` tag.

## Summary

Adds one read-only method:

```rust
impl LoroText {
    pub fn history_order(&self) -> LoroResult<Vec<TextHistorySpan>>;
}

pub struct TextHistorySpan {
    pub id: ID,        // first character; the run's counters are consecutive
    pub text: String,  // the run's characters, as inserted
    pub deleted: bool, // deleted at the oplog's latest version
}
```

It returns every character the text container has ever held, deleted ones
included, in the container's Fugue order: the single total order over all
inserted characters from which the text at any version is a subsequence.

## Motivation

Applications that render a text "with some changes left out" (review views,
suggestion modes, blame with a change excluded, selective undo previews) need
to show characters that were inserted by kept changes and not deleted by kept
changes, in the order the CRDT gives them. Loro already computes that order
internally (the richtext tracker that diff calculation builds), but it is not
reachable from the public API. Without it, callers rebuild it indirectly:
forking at several versions, diffing per change, and re-inserting deleted
runs with cursor anchors. That is quadratic and cannot place a restored
character correctly relative to other deleted characters in the same gap,
because the visible text at any one version no longer contains them.

With the order exposed, such a view is one pass: filter `history_order()` by
the caller's rule and concatenate.

## Implementation

- `Tracker::text_runs_in_order` (internal) iterates the tracker's rope and
  yields text runs with their real ids and deletion status, skipping style
  anchors, move anchors, the unknown placeholder and future spans.
- `diff_calc::text_history_runs` (internal) reuses
  `RichtextDiffCalculator::build_full_crdt_tracker`, the same replay a
  checkout from the empty version performs, then checks the tracker out at the
  oplog version, and reads each run's text from the tracker's own text table.
- `TextHandler::history_order` builds the spans from those runs,
  merging neighbours with consecutive ids and equal status, and re-exports
  `TextHistorySpan` from `loro`.

Nothing about state, encoding, import or checkout changes. The method only
locks the oplog.

Cost: one replay of the document's changes for this container, roughly a
checkout from the empty version, `O(ops log ops)`.

## Limits

- Committed history only: a pending transaction is not included.
- `deleted` is relative to the oplog's latest version, not to a detached
  checkout.
- Detached containers and shallow documents return an error, since they have
  no history (or none before the shallow root). Supporting shallow documents
  would need the seeded root spans' real ids; left for a follow-up.
- Style anchors are not reported. A follow-up could expose them if a rich-text
  consumer needs mark positions in the same order.
- Rust only; a `loro-wasm` binding can follow if wanted.

## Tests

`crates/loro/tests/text_history_order.rs`:

- with no deletions, the order is the visible text, and each id matches the
  cursor id at that position;
- deleted characters keep their original ids and sit where they were, and
  later insertions into the gap are ordered by Fugue;
- a concurrent insertion into a deleted gap, later deleted too, keeps one
  order on both replicas;
- style anchors are left out; detached containers and shallow documents error;
- randomized: three peers, concurrent edits synced in different orders. Every
  replica, and one rebuilt from a snapshot, returns the same runs; the
  non-deleted runs are the text; and at every recorded earlier version
  (`fork_at`) the order equals the full order restricted to the characters
  that version has.

Plus the doctest on `LoroText::history_order`.
