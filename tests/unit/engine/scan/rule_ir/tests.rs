use super::*;

#[test]
fn build_clue_index_sorts_overlapping_hits_by_start_offset() {
    let ac = AhoCorasickBuilder::new()
        .match_kind(MatchKind::Standard)
        .build(["aba", "ba", "a"])
        .expect("build clue AC");
    let mut index = Vec::new();

    build_clue_index_into(Some(&ac), "aba", &mut index);

    assert!(
        index.windows(2).all(|w| w[0].0 <= w[1].0),
        "clue hits must be sorted by start offset: {index:?}"
    );
}

#[test]
fn lookup_clues_counts_distinct_positive_ids_across_overlaps() {
    let clue_index = vec![(0, 2), (0, 2), (1, 3), (2, 5)];
    let pos_ids = vec![2, 3, 5];

    let (pos_found, any_neg) = lookup_clues_in_window(&clue_index, 0, 3, Some(&pos_ids), None);

    assert_eq!(pos_found, 3, "should count distinct positive clue ids");
    assert!(!any_neg, "no negative clues should be reported");
}

#[test]
fn lookup_clues_negative_hit_vetoes_same_offset_window() {
    let clue_index = vec![(0, 2), (0, 7), (1, 3)];
    let pos_ids = vec![2, 3];
    let neg_ids = vec![7];

    let (pos_found, any_neg) =
        lookup_clues_in_window(&clue_index, 0, 2, Some(&pos_ids), Some(&neg_ids));

    assert_eq!(
        pos_found, 1,
        "positive clue before veto should still be counted"
    );
    assert!(any_neg, "negative clue at same offset must veto");
}
