//! `LoroText::history_order`: every character a text has held, in Fugue order.

use loro::{ExportMode, Frontiers, LoroDoc, LoroText, TextHistorySpan, ID};
use rand::{rngs::StdRng, Rng, SeedableRng};

/// One entry per character: (id, char, deleted).
fn chars(spans: &[TextHistorySpan]) -> Vec<(ID, char, bool)> {
    spans
        .iter()
        .flat_map(|span| {
            span.text
                .chars()
                .enumerate()
                .map(move |(i, ch)| (span.id.inc(i as i32), ch, span.deleted))
        })
        .collect()
}

fn visible(spans: &[TextHistorySpan]) -> String {
    spans
        .iter()
        .filter(|span| !span.deleted)
        .map(|span| span.text.as_str())
        .collect()
}

fn order(text: &LoroText) -> Vec<TextHistorySpan> {
    text.history_order().unwrap()
}

#[test]
fn without_deletions_the_order_is_the_visible_text() {
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "wörld 🌍").unwrap();
    text.insert(0, "hello ").unwrap();
    text.insert(5, ",").unwrap();
    doc.commit();
    let spans = order(&text);
    assert!(spans.iter().all(|span| !span.deleted));
    let all: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(all, text.to_string());
    assert_eq!(all, "hello, wörld 🌍");
    // Every character's id is the one a cursor reports for it.
    for (position, (id, _, _)) in chars(&spans).into_iter().enumerate() {
        let cursor = text.get_cursor(position, Default::default()).unwrap();
        assert_eq!(cursor.id, Some(id));
    }
}

#[test]
fn an_empty_or_never_written_text_has_no_history() {
    let doc = LoroDoc::new();
    assert!(order(&doc.get_text("text")).is_empty());
    let text = doc.get_text("text");
    text.insert(0, "abc").unwrap();
    text.delete(0, 3).unwrap();
    doc.commit();
    let spans = order(&text);
    assert_eq!(visible(&spans), "");
    assert_eq!(
        chars(&spans).into_iter().map(|c| c.1).collect::<String>(),
        "abc"
    );
}

#[test]
fn deleted_characters_sit_where_they_were() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "hello world").unwrap();
    doc.commit();
    text.delete(3, 4).unwrap(); // "lo w"
    doc.commit();
    text.insert(3, "p").unwrap(); // typed where the deletion was
    doc.commit();
    assert_eq!(text.to_string(), "helporld");
    let spans = order(&text);
    assert_eq!(visible(&spans), text.to_string());
    let all: String = chars(&spans).into_iter().map(|c| c.1).collect();
    // "p" was inserted after "hel"'s last visible "l", left of the deleted run.
    assert_eq!(all, "helplo world");
    let deleted: String = chars(&spans)
        .into_iter()
        .filter(|c| c.2)
        .map(|c| c.1)
        .collect();
    assert_eq!(deleted, "lo w");
    // Deleted characters keep their original ids.
    let lo_w: Vec<_> = chars(&spans).into_iter().filter(|c| c.2).collect();
    assert_eq!(
        lo_w.iter().map(|c| c.0).collect::<Vec<_>>(),
        (3..7).map(|c| ID::new(1, c)).collect::<Vec<_>>()
    );
}

#[test]
fn concurrent_inserts_into_a_deleted_gap_keep_one_order() {
    // Author deletes "q"; someone concurrently types "p" after "q"; then the
    // author deletes "p". Both deleted characters stay in the order, "q" first.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_text("text").insert(0, "aqz").unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.get_text("text").delete(1, 1).unwrap();
    a.commit();
    b.get_text("text").insert(2, "p").unwrap();
    b.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(a.get_text("text").to_string(), "apz");
    a.get_text("text").delete(1, 1).unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let spans = order(&a.get_text("text"));
    assert_eq!(spans, order(&b.get_text("text")));
    let all: Vec<_> = chars(&spans).into_iter().map(|c| (c.1, c.2)).collect();
    assert_eq!(
        all,
        vec![('a', false), ('q', true), ('p', true), ('z', false)]
    );
}

#[test]
fn style_anchors_are_not_characters() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "bold move").unwrap();
    text.mark(0..4, "bold", true).unwrap();
    text.delete(4, 1).unwrap();
    doc.commit();
    let spans = order(&text);
    assert_eq!(visible(&spans), "boldmove");
    assert_eq!(
        chars(&spans).into_iter().map(|c| c.1).collect::<String>(),
        "bold move"
    );
}

#[test]
fn a_detached_text_has_no_history() {
    assert!(LoroText::new().history_order().is_err());
}

#[test]
fn a_shallow_document_is_refused() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "abc").unwrap();
    doc.commit();
    text.insert(3, "def").unwrap();
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    let shallow = LoroDoc::new();
    shallow
        .import(
            &doc.export(ExportMode::shallow_snapshot(&frontiers))
                .unwrap(),
        )
        .unwrap();
    assert!(shallow.get_text("text").history_order().is_err());
}

/// Random concurrent edits on several peers, synced in different orders. The
/// order is the same on every replica, filtering it by "not deleted" gives
/// the text, and at every earlier version the order is the full order
/// restricted to the characters that version has.
#[test]
fn the_order_is_deterministic_and_stable_across_versions() {
    for seed in 0..40u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let peers: Vec<LoroDoc> = (0..3)
            .map(|i| {
                let doc = LoroDoc::new();
                doc.set_peer_id(i + 1).unwrap();
                doc
            })
            .collect();
        let mut checkpoints: Vec<Frontiers> = Vec::new();
        let alphabet: Vec<char> = "abcxyzé🌊ß".chars().collect();
        for _ in 0..60 {
            let doc = &peers[rng.gen_range(0..peers.len())];
            let text = doc.get_text("text");
            let len = text.len_unicode();
            if len > 0 && rng.gen_bool(0.4) {
                let pos = rng.gen_range(0..len);
                let del = rng.gen_range(1..=(len - pos).min(4));
                text.delete(pos, del).unwrap();
            } else {
                let pos = rng.gen_range(0..=len);
                let s: String = (0..rng.gen_range(1..4))
                    .map(|_| alphabet[rng.gen_range(0..alphabet.len())])
                    .collect();
                text.insert(pos, &s).unwrap();
            }
            doc.commit();
            if rng.gen_bool(0.3) {
                let from = &peers[rng.gen_range(0..peers.len())];
                let to = &peers[rng.gen_range(0..peers.len())];
                to.import(&from.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
            }
            if rng.gen_bool(0.2) {
                checkpoints.push(doc.oplog_frontiers());
            }
        }
        // Sync everyone, in different orders.
        for (i, to) in peers.iter().enumerate() {
            for j in 0..peers.len() {
                let from = &peers[(i + j) % peers.len()];
                to.import(&from.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
            }
        }
        let full = order(&peers[0].get_text("text"));
        for doc in &peers {
            assert_eq!(order(&doc.get_text("text")), full, "seed {seed}");
            assert_eq!(visible(&full), doc.get_text("text").to_string());
        }
        // A replica built from a snapshot agrees too.
        let restored =
            LoroDoc::from_snapshot(&peers[1].export(ExportMode::Snapshot).unwrap()).unwrap();
        assert_eq!(order(&restored.get_text("text")), full, "seed {seed}");

        let full_chars = chars(&full);
        for frontiers in &checkpoints {
            let old = peers[0].fork_at(frontiers).unwrap();
            let vv = old.oplog_vv();
            let expected: Vec<(ID, char)> = full_chars
                .iter()
                .filter(|(id, _, _)| vv.includes_id(*id))
                .map(|(id, ch, _)| (*id, *ch))
                .collect();
            let old_chars = chars(&order(&old.get_text("text")));
            assert_eq!(
                old_chars.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
                expected,
                "seed {seed}"
            );
            let old_visible: String = old_chars.iter().filter(|c| !c.2).map(|c| c.1).collect();
            assert_eq!(old_visible, old.get_text("text").to_string(), "seed {seed}");
        }
    }
}
