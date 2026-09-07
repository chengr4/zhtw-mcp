use super::*;

// Every blank-line form has to split, including the mixed ones a patch or a
// merge leaves behind. Matching only "\n\n" and "\r\n\r\n" let a mixed document
// read as one paragraph, and the paragraph-level detectors returned under their
// minimum count without a word.
#[test]
fn a_blank_line_splits_whatever_terminators_it_uses() {
    for sep in ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"] {
        let doc = format!("第一段。{sep}第二段。{sep}第三段。");
        let paras = split_paragraphs(&doc);
        assert_eq!(
            paras.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
            ["第一段。", "第二段。", "第三段。"],
            "separator {sep:?}"
        );
    }
}

// The phrase-density signal reads the text, not the issue list, so a document
// that quotes a tell had every finding suppressed and still scored for the
// phrase: "No issues found" beside "AI score: 0.92".
#[test]
fn a_quoted_phrase_scores_no_higher_than_its_absence() {
    let body = "這裡示範一個常見的寫作毛病。".repeat(30);
    let quoted = "避免使用「值得注意的是」這個詞。".repeat(8);
    let used = "值得注意的是，這個設計很好。".repeat(8);

    let score = |doc: &str| {
        let scanner = Scanner::new(
            crate::rules::loader::load_embedded_ruleset()
                .unwrap()
                .spelling_rules,
            Vec::new(),
        );
        let mut cfg = Profile::Base.config();
        cfg.ai_filler_detection = true;
        cfg.ai_density_detection = true;
        cfg.ai_structural_patterns = true;
        cfg.ai_semantic_safety = true;
        scanner
            .scan_with_config(doc, &[], cfg)
            .ai_signature
            .map_or(0.0, |r| r.score)
    };

    let quoting = score(&format!("{body}\n\n{quoted}\n\n{body}"));
    let using = score(&format!("{body}\n\n{used}\n\n{body}"));
    assert!(
        quoting < using,
        "quoting a tell must score below using it: {quoting} vs {using}"
    );
}
