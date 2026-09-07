use super::*;

/// Feed one document's worth of HTML and report the ranges. Real callers
/// feed only the parser's HTML events; feeding the whole string is the
/// same thing for these fixtures, which are HTML end to end.
fn scopes(text: &str) -> Vec<ByteRange> {
    let mut tracker = LangScopes::new();
    tracker.feed(text, 0);
    tracker.finish(text.len())
}

/// The substrings the tracker would keep the scanner away from.
fn excluded_text(text: &str) -> Vec<&str> {
    scopes(text).iter().map(|r| &text[r.start..r.end]).collect()
}

#[test]
fn chinese_primary_subtags() {
    for tag in ["zh", "zh-TW", "zh-Hant-TW", "ZH_CN", "yue-Hant", " zh "] {
        assert!(is_chinese_lang(tag), "{tag} should read as Chinese");
    }
    for tag in ["", "en", "en-US", "ja", "zhx", "z"] {
        assert!(!is_chinese_lang(tag), "{tag} should not read as Chinese");
    }
}

// Depth cap. Every search in the tracker is bounded by the open-element stack,
// so what these check is that the stack is bounded by MAX_DEPTH rather than by
// the document, and that the cap gives up in the direction that leaves text
// scanned.

#[test]
fn a_declaration_inside_the_cap_still_scopes() {
    // One short of the cap, so the declaration is tracked as usual. The scope
    // has to reach its own closer and no further.
    let depth = MAX_DEPTH - 2;
    let text = format!(
        "{}<span lang=\"en\">b</span>{}c",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    );
    let ranges = scopes(&text);
    assert_eq!(ranges.len(), 1, "one declaration, one range");
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<span lang=\"en\">b</span>"
    );
}

#[test]
fn a_declaration_past_the_cap_scopes_nothing() {
    // Giving up has to leave the text scanned rather than silently excluded, so
    // the run under an untracked declaration is not reported.
    let text = format!(
        "{}<span lang=\"en\">b</span>",
        "<div>".repeat(MAX_DEPTH + 10)
    );
    assert!(
        scopes(&text).is_empty(),
        "a declaration past the cap must not take text out of the scan"
    );
}

#[test]
fn tracking_resumes_once_the_nesting_unwinds() {
    // The counter balances what it suppressed, so a declaration written after
    // the deep run comes back is tracked again. Without that, one deep spot
    // would disable scoping for the rest of the document.
    let deep = format!(
        "{}{}",
        "<div>".repeat(MAX_DEPTH + 10),
        "</div>".repeat(MAX_DEPTH + 10)
    );
    let text = format!("{deep}<span lang=\"en\">b</span>c");
    let ranges = scopes(&text);
    assert_eq!(ranges.len(), 1, "scoping did not resume after the deep run");
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<span lang=\"en\">b</span>"
    );
}

#[test]
fn deep_nesting_does_not_walk_the_whole_stack_per_tag() {
    // A stack of transparent elements over one paragraph defeats the cheap
    // "nothing is closable" exit, so this shape is what the cap itself has to
    // bound. The assertion is only that it terminates and stays conservative;
    // the cost is what the cap is for.
    let n = 20_000;
    let text = format!("<p>{}{}", "<span>".repeat(n), "<b>".repeat(n));
    assert!(scopes(&text).is_empty(), "nothing declared a lang");
}

#[test]
fn a_new_row_ends_the_row_before_it() {
    // A row starting inside an open cell ends the cell and then the row, the
    // way a browser does. Closing only the cell left the first row open, and
    // its lang went on scoping the second row's text.
    assert_eq!(
        excluded_text("<table><tr lang=\"en\"><td>EN<tr><td>ZH</table>"),
        vec!["<tr lang=\"en\"><td>EN<tr>"]
    );
}

#[test]
fn a_new_cell_ends_the_cell_before_it() {
    assert_eq!(
        excluded_text("<table><tr><td lang=\"en\">EN<td>ZH</table>"),
        vec!["<td lang=\"en\">EN<td>"]
    );
}

#[test]
fn a_column_group_ends_at_the_first_thing_that_is_not_a_col() {
    // colgroup's end tag is optional, so the table body ends it. Left open, a
    // lang on it scoped the rest of the table.
    assert_eq!(
        excluded_text("<table><colgroup lang=\"en\"><tbody><tr><td>ZH</table>"),
        vec!["<colgroup lang=\"en\"><tbody>"]
    );
}

#[test]
fn a_column_group_does_not_outlive_a_tag_it_cannot_close_through() {
    // Exempting the tags "in column group" hands to another insertion mode was
    // tried and reverted. Either one is a barrier the implicit-close walk
    // cannot see past, so the group stayed open and its foreign lang took the
    // rest of the table out of the scan. Ending the scope early is the
    // inaccuracy this file can afford; dropping prose silently is not.
    for markup in [
        "<table><colgroup lang=\"en\"><html><tbody><tr><td>ZH</table>",
        "<table><colgroup lang=\"en\"><template><tbody><tr><td>ZH</table>",
    ] {
        let covered = excluded_text(markup).concat();
        assert!(
            !covered.contains("ZH"),
            "the table tail was excluded by the column group's lang: {covered}"
        );
    }
}

#[test]
fn a_column_group_survives_its_own_cols() {
    let text = "<table><colgroup lang=\"en\"><col><col><tbody><tr><td>ZH</table>";
    assert_eq!(
        excluded_text(text),
        vec!["<colgroup lang=\"en\"><col><col><tbody>"]
    );
}

#[test]
fn a_repeated_lang_keeps_the_first_one() {
    // HTML drops a duplicate attribute, so the first value is what a browser
    // parses. Taking the last one inverted both of these.
    assert!(
        !excluded_text("<span lang=\"en\" lang=\"zh-TW\">x</span>").is_empty(),
        "the first lang, en, decides: the run is foreign"
    );
    assert!(
        excluded_text("<span lang=\"zh-TW\" lang=\"en\">x</span>").is_empty(),
        "the first lang, zh-TW, decides: the run stays scanned"
    );
}

#[test]
fn inline_span_scopes_its_text() {
    assert_eq!(
        excluded_text("a<span lang=\"en\">b</span>c"),
        vec!["<span lang=\"en\">b</span>"]
    );
}

#[test]
fn chinese_span_is_not_scoped() {
    assert!(scopes("a<span lang=\"zh-TW\">中文</span>c").is_empty());
}

#[test]
fn nested_same_name_span_does_not_close_early() {
    let text = "<span lang=\"en\">a<span>b</span>c</span>d";
    assert_eq!(
        excluded_text(text),
        vec!["<span lang=\"en\">a<span>b</span>c</span>"]
    );
}

#[test]
fn chinese_span_reopens_scanning_inside_an_english_scope() {
    let text = "<div lang=\"en\">A<span lang=\"zh-TW\">中</span>B</div>";
    let ranges = scopes(text);
    assert_eq!(ranges.len(), 2);
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<div lang=\"en\">A<span lang=\"zh-TW\">"
    );
    assert_eq!(&text[ranges[1].start..ranges[1].end], "</span>B</div>");
}

#[test]
fn empty_lang_reads_as_unknown_not_foreign() {
    assert!(scopes("<span lang=\"\">中文</span>").is_empty());
    assert!(scopes("<span lang>中文</span>").is_empty());
    // And it stops an outer declaration from reaching the inner run.
    let text = "<div lang=\"en\">A<span lang=\"\">中</span></div>";
    assert_eq!(scopes(text).len(), 2);
}

#[test]
fn unclosed_tag_scopes_to_end_of_input() {
    let text = "a<span lang=\"en\">b";
    assert_eq!(excluded_text(text), vec!["<span lang=\"en\">b"]);
}

#[test]
fn unmatched_closer_is_ignored() {
    assert!(scopes("</span>中文").is_empty());
    let text = "<div lang=\"en\">a</span>b</div>";
    assert_eq!(
        excluded_text(text),
        vec!["<div lang=\"en\">a</span>b</div>"]
    );
}

#[test]
fn void_element_scopes_nothing() {
    assert!(scopes("<br lang=\"en\">中文").is_empty());
    assert!(scopes("<img lang=\"en\" src=\"a.png\">中文").is_empty());
}

#[test]
fn self_closing_element_scopes_nothing() {
    assert!(scopes("<span lang=\"en\" />中文").is_empty());
}

#[test]
fn a_tag_written_inside_a_script_is_a_string_not_an_element() {
    assert!(scopes("<script>var s = \"<span lang='en'>\";</script>中文").is_empty());
    // Even unclosed, so it cannot silence the rest of the document.
    assert!(scopes("<script>\"<span lang='en'>\"</script>\n中文, 對").is_empty());
}

#[test]
fn a_raw_text_element_can_span_feeds() {
    let mut tracker = LangScopes::new();
    tracker.feed("<script>", 0);
    tracker.feed("<span lang=\"en\">", 8);
    tracker.feed("</script>", 24);
    assert!(tracker.finish(100).is_empty());
}

#[test]
fn a_scope_opened_before_a_script_survives_it() {
    let text = "<div lang=\"en\">a<script>x</script>b</div>c";
    assert_eq!(
        excluded_text(text),
        vec!["<div lang=\"en\">a<script>x</script>b</div>"]
    );
}

#[test]
fn a_close_tag_written_inside_a_script_does_not_pop_the_real_stack() {
    // The "</div>" here is a string. Reading it as markup would end the English
    // scope early and scan the run the author marked as English.
    let text = "<div lang=\"en\"><script>const x = \"</div>\";</script>ok</div>後\n";
    assert_eq!(
        excluded_text(text),
        vec!["<div lang=\"en\"><script>const x = \"</div>\";</script>ok</div>"]
    );
}

#[test]
fn cdata_ends_at_its_own_closer_not_the_first_angle_bracket() {
    assert!(scopes("<![CDATA[ a > b <span lang=\"en\"> ]]>中文").is_empty());
}

#[test]
fn a_second_paragraph_closes_the_first() {
    // p has an optional end tag, so the zh-TW paragraph is a sibling of the
    // English one, not a child of it, and the tail is outside both.
    let text = "<p lang=\"en\">English, here<p lang=\"zh-TW\">中文, 這裡</p>中文, 那裡";
    let ranges = scopes(text);
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<p lang=\"en\">English, here<p lang=\"zh-TW\">"
    );
}

#[test]
fn list_items_and_table_cells_close_on_their_next_sibling() {
    let text = "<ul><li lang=\"en\">one<li>中文, 這裡</ul>";
    let ranges = scopes(text);
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<li lang=\"en\">one<li>"
    );

    let cells = "<table><tr><td lang=\"en\">one<td>中文, 這裡</table>";
    let ranges = scopes(cells);
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        &cells[ranges[0].start..ranges[0].end],
        "<td lang=\"en\">one<td>"
    );
}

#[test]
fn an_inline_element_does_not_hide_the_paragraph_it_sits_in() {
    // The span is between the two paragraphs on the stack, and looking only at
    // the top would leave the English one open across the second.
    let text = "<p lang=\"en\">one<span><p>中文, 這裡";
    let ranges = scopes(text);
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        &text[ranges[0].start..ranges[0].end],
        "<p lang=\"en\">one<span><p>"
    );
}

#[test]
fn a_block_element_in_between_does_not_stop_the_implicit_close() {
    // HTML closes the first item when the second starts, div or no div.
    let text = "<ul><li lang=\"en\">one<div>more<li>中文, 這裡</ul>";
    assert_eq!(
        excluded_text(text),
        vec!["<li lang=\"en\">one<div>more<li>"]
    );

    let cells = "<table><tr><td lang=\"en\">one<div>more<td>中文, 這裡</table>";
    assert_eq!(
        excluded_text(cells),
        vec!["<td lang=\"en\">one<div>more<td>"]
    );

    let terms = "<dl><dt lang=\"en\">one<div>more<dd>中文, 這裡</dl>";
    assert_eq!(
        excluded_text(terms),
        vec!["<dt lang=\"en\">one<div>more<dd>"]
    );
}

#[test]
fn a_special_element_in_between_stops_the_implicit_close() {
    // Not an oversight in the list above: HTML's li algorithm exempts address,
    // div and p from the special category and nothing else, so a section
    // between two items leaves the second nested in the first and the outer
    // declaration still applies to it.
    let text = "<ul><li lang=\"en\">one<section>more<li>two</ul>後";
    assert_eq!(
        excluded_text(text),
        vec!["<li lang=\"en\">one<section>more<li>two</ul>"]
    );
}

#[test]
fn a_nested_table_or_list_stops_the_implicit_close() {
    // The inner cell belongs to the inner table, so it does not end the cell
    // the outer one sits in.
    let text = "<td lang=\"en\">one<table><tr><td>two</table>three</td>中文";
    assert_eq!(
        excluded_text(text),
        vec!["<td lang=\"en\">one<table><tr><td>two</table>three</td>"]
    );

    let list = "<li lang=\"en\">one<ul><li>two</ul>three</li>中文";
    assert_eq!(
        excluded_text(list),
        vec!["<li lang=\"en\">one<ul><li>two</ul>three</li>"]
    );
}

#[test]
fn a_raw_text_element_still_scopes_its_own_lang() {
    let text = "<title lang=\"en\">Some, title</title>中文, 這裡";
    assert_eq!(
        excluded_text(text),
        vec!["<title lang=\"en\">Some, title</title>"]
    );
}

#[test]
fn an_inline_element_does_not_close_a_paragraph() {
    let text = "<p lang=\"en\">one<span>two</span>three</p>中文";
    assert_eq!(
        excluded_text(text),
        vec!["<p lang=\"en\">one<span>two</span>three</p>"]
    );
}

#[test]
fn comment_contents_do_not_open_a_scope() {
    assert!(scopes("<!-- <span lang=\"en\"> -->中文").is_empty());
}

#[test]
fn attribute_forms_and_case() {
    for text in [
        "<SPAN LANG=EN>x</SPAN>",
        "<span lang = 'en'>x</span>",
        "<span class=\"a>b\" lang=\"en\">x</span>",
        "<span data-x lang=\"en\">x</span>",
    ] {
        assert_eq!(scopes(text).len(), 1, "{text} should scope its text");
    }
}

#[test]
fn a_greater_than_inside_a_quoted_value_does_not_end_the_tag() {
    let text = "<span title=\"a>b\" lang=\"en\">x</span>y";
    assert_eq!(
        excluded_text(text),
        vec!["<span title=\"a>b\" lang=\"en\">x</span>"]
    );
}

#[test]
fn base_offset_is_added() {
    let mut tracker = LangScopes::new();
    tracker.feed("<span lang=\"en\">", 100);
    tracker.feed("</span>", 130);
    let ranges = tracker.finish(200);
    assert_eq!(
        ranges,
        vec![ByteRange {
            start: 100,
            end: 137
        }]
    );
}

#[test]
fn non_tag_angle_bracket_is_skipped() {
    assert!(scopes("1 < 2 and 3 > 2，對吧").is_empty());
}
