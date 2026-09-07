use super::*;

fn score(text: &str) -> Option<TranslationeseReport> {
    compute_translationese_score(text, &[], &[])
}

#[test]
fn short_text_returns_none() {
    assert!(score("短文").is_none());
}

#[test]
fn clean_text_scores_low() {
    // Natural zh-TW prose, no translationese patterns (>200 chars).
    let text = "台灣是一個美麗的島嶼。\
                這裡有豐富的自然景觀和人文風情。\
                山脈縱貫全島，海岸線變化多端。\
                人民友善熱情，文化多元而包容。\
                教育普及，科技產業蓬勃發展。\
                美食種類繁多，從夜市小吃到精緻料理。\
                四季分明的氣候適合各種戶外活動。\
                歷史悠久的寺廟和現代建築並存。\
                交通便利，鐵路和公路網路完善。\
                醫療水準在亞洲名列前茅。\
                城市規劃相當完善，公共運輸覆蓋率極高。\
                都市農業開始萌芽，屋頂菜園數量逐年增加。\
                博物館藏品豐富，展覽內容定期更新。\
                圖書館遍布各區，閱讀風氣盛行。\
                社區活動中心提供多樣化課程。\
                夜市文化獨樹一幟，吸引各國觀光客。\
                傳統節慶慶典保留完整的民俗活動。\
                志工服務精神深入民間組織運作。\
                環保意識逐年提升，垃圾分類成效顯著。\
                全民健保制度獲得國際社會高度肯定。";
    let report = score(text);
    assert!(report.is_some());
    let r = report.unwrap();
    assert!(
        r.score < 0.3,
        "Clean text should score low, got {}",
        r.score
    );
}

#[test]
fn westernized_text_scores_higher() {
    // Text with heavy translationese markers (>200 chars).
    let text = "在這個過程中，問題被充分地討論了。\
                她被認為是最優秀的人選。\
                政府進行了全面的調查和分析。\
                他們對這個問題加以研究和討論。\
                這個方案被廣泛認為是最好的選擇。\
                她被授予了最高榮譽的獎項。\
                他們進行了長時間的討論和辯論。\
                結果被公布在最新的報告中。\
                這些措施被視為非常必要的步驟。\
                整個計劃被認為是成功的典範。\
                他們的努力被證明是值得的。\
                她被選為年度最佳員工的候選人。\
                這項政策被認為對經濟發展有重大影響。\
                他們進行了深入的市場分析和評估。\
                這個決定被視為具有里程碑意義的轉折。\
                她被指派負責整個專案的執行工作。\
                他們加以整合並予以重新規劃。\
                這些成果被廣泛報導和討論。\
                她的表現被評價為出色的領導典範。\
                他們對問題進行了全面的檢討和改善。";
    let report = score(text);
    assert!(report.is_some());
    let r = report.unwrap();
    assert!(
        r.score > 0.0,
        "Westernized text should score higher, got {}",
        r.score
    );
}

#[test]
fn technical_domain_scores_lower_than_literary_on_passive_text() {
    // Identical text scored under technical (lenient) vs literary (strict)
    // domains: literary should always score >= technical for the same
    // passive-heavy input.
    let text = "他被認為是優秀的學者。她被視為傑出領袖。整個項目被認為是成功的。\
                她被授予了榮譽。她被選為主席。他們進行了討論。\
                她被廣泛認為是優秀的人選。他被任命為主管。\
                他被指派負責這個專案。他們進行了完整的評估。\
                這個方案被認為是最好的選擇。他們予以支援。\
                這些成果被報導出來。她被評為年度典範。\
                他被推舉為代表。整體計畫被視為一大進展。\
                研究結果被廣泛發表並被多次引用。這個提案被多方採納。\
                他們的努力被證明是值得的。她被選為年度最佳員工。\
                這項政策被認為對發展有重大影響。\
                她被授予最高榮譽。他被廣泛認可為傑出領袖。"; // >200 chars
    let r_tech =
        compute_translationese_score_with_domain(text, &[], &[], TranslationeseDomain::Technical)
            .expect("technical score");
    let r_lit =
        compute_translationese_score_with_domain(text, &[], &[], TranslationeseDomain::Literary)
            .expect("literary score");
    assert!(
        r_lit.score >= r_tech.score,
        "literary ({}) should score >= technical ({}) on passive-heavy text",
        r_lit.score,
        r_tech.score
    );
    assert_eq!(r_tech.domain, TranslationeseDomain::Technical);
    assert_eq!(r_lit.domain, TranslationeseDomain::Literary);
}

#[test]
fn domain_from_str_round_trips() {
    for d in [
        TranslationeseDomain::General,
        TranslationeseDomain::Technical,
        TranslationeseDomain::Literary,
        TranslationeseDomain::News,
    ] {
        assert_eq!(TranslationeseDomain::from_str_strict(d.name()), Some(d));
    }
    assert_eq!(TranslationeseDomain::from_str_strict("invalid"), None);
}

#[test]
fn de_chain_detection() {
    let chain = compute_max_de_chain("這是我的朋友的妹妹的同學的書", &[]);
    assert_eq!(chain, 4);
}

#[test]
fn passive_count_basic() {
    let count = count_pattern("他被打了，她也被罵了", "被", &[]);
    assert_eq!(count, 2);
}

#[test]
fn weak_verb_count_basic() {
    let count = count_weak_verbs("我們進行討論並加以分析", &[]);
    assert_eq!(count, 2);
}

#[test]
fn weak_verb_count_rejects_bare_prefix() {
    // "進行" alone = "in progress" (legitimate standalone use); must not
    // inflate the weak-verb signal.
    assert_eq!(count_weak_verbs("專案正在進行，尚未完成。", &[]), 0);
    // "進行中" likewise has no nominalized object.
    assert_eq!(count_weak_verbs("工作進行中，請稍候。", &[]), 0);
}

#[test]
fn weak_verb_count_allows_intervening_particles() {
    // "進行了討論": the 了 between prefix and object should still count.
    assert_eq!(count_weak_verbs("他們進行了討論並加以了分析", &[]), 2);
}

#[test]
fn weak_verb_count_skips_excluded_object() {
    // Codex round 4: an object span inside an exclusion zone (e.g. inline code)
    // must not count toward the weak-verb signal even when the prefix itself is
    // in clean prose.
    let text = "他們進行討論之後，我們進行討論再次回顧。";
    // Mark the second "討論" (bytes 33..39 in this string) as excluded.
    let second_obj_start = text.rfind("討論").unwrap();
    let excluded = vec![ByteRange {
        start: second_obj_start,
        end: second_obj_start + "討論".len(),
    }];

    // Without the fix: count = 2. With the fix: only the unexcluded first
    // occurrence counts → 1.
    assert_eq!(count_weak_verbs(text, &excluded), 1);
}

#[test]
fn report_deserializes_without_domain_field() {
    // Codex round 4: pre-domain cache entries must still load. Without
    // serde(default) on domain, a single old entry would cause load_entries()
    // to discard the whole cache file.
    let json = r#"{
        "score": 0.5,
        "markers": [],
        "top_signals": [],
        "max_de_chain": 0
    }"#;
    let r: TranslationeseReport =
        serde_json::from_str(json).expect("missing domain field must default, not fail");
    assert_eq!(r.domain, TranslationeseDomain::General);
}
