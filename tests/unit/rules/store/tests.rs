use super::*;
use crate::rules::ruleset::RuleType;

fn sample_base_spelling() -> Vec<SpellingRule> {
    vec![
        SpellingRule::new("軟件", vec!["軟體".into()], RuleType::CrossStrait),
        SpellingRule::new("內存", vec!["記憶體".into()], RuleType::CrossStrait),
    ]
}

fn sample_base_case() -> Vec<CaseRule> {
    vec![CaseRule {
        term: "JavaScript".into(),
        alternatives: Some(vec!["javascript".into()]),
        disabled: false,
    }]
}

#[test]
fn load_base_rules_without_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let store = OverrideStore::open(&path).unwrap();

    let spelling = store.load_spelling_rules(&sample_base_spelling());
    assert_eq!(spelling.len(), 2);
    assert_eq!(spelling[0].from, "軟件");

    let case = store.load_case_rules(&sample_base_case());
    assert_eq!(case.len(), 1);
    assert_eq!(case[0].term, "JavaScript");
}

#[test]
fn spelling_override_upsert_and_merge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();

    // Override existing rule.
    let override_rule = SpellingRule::new(
        "軟件",
        vec!["軟體".into(), "應用程式".into()],
        RuleType::CrossStrait,
    );
    store.upsert_spelling_override(&override_rule).unwrap();

    let rules = store.load_spelling_rules(&sample_base_spelling());
    assert_eq!(rules.len(), 2);
    let r = rules.iter().find(|r| r.from == "軟件").unwrap();
    assert_eq!(r.to.len(), 2);

    // Add new override.
    let new_rule = SpellingRule::new("視頻", vec!["影片".into()], RuleType::CrossStrait);
    store.upsert_spelling_override(&new_rule).unwrap();

    let rules = store.load_spelling_rules(&sample_base_spelling());
    assert_eq!(rules.len(), 3);
}

#[test]
fn disable_builtin_spelling_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();
    let base = sample_base_spelling();

    assert_eq!(store.load_spelling_rules(&base).len(), 2);

    store.disable_spelling_rule("軟件").unwrap();
    let rules = store.load_spelling_rules(&base);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].from, "內存");

    // Re-enable by deleting the override.
    store.delete_spelling_override("軟件").unwrap();
    assert_eq!(store.load_spelling_rules(&base).len(), 2);
}

#[test]
fn a_disabled_tag_retires_a_whole_family() {
    let dir = tempfile::tempdir().unwrap();
    let base = crate::rules::loader::load_embedded_ruleset()
        .unwrap()
        .spelling_rules;

    // Enabled only: the merge already drops watchlisted rules, so they cannot
    // be dropped a second time by the tag.
    let tagged = base
        .iter()
        .filter(|r| {
            !r.disabled
                && r.tags
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|x| x == "src:humanizer"))
        })
        .count();
    assert!(tagged > 1, "fixture needs a family, got {tagged}");

    let store = OverrideStore::open(&dir.path().join("o.json")).unwrap();
    let packs = PackStore::new(dir.path().join("packs"));
    let (kept, _) = build_merged_rules(&base, &[], &store, &packs, &[]);
    let before = kept.len();

    let mut off = OverrideStore::open(&dir.path().join("off.json")).unwrap();
    off.set_disabled_tags(vec!["src:humanizer".into()]).unwrap();
    let (after, _) = build_merged_rules(&base, &[], &off, &packs, &[]);

    assert_eq!(
        before - after.len(),
        tagged,
        "disabling a tag must drop exactly the rules carrying it"
    );
    assert!(!after.iter().any(|r| r
        .tags
        .as_ref()
        .is_some_and(|t| t.iter().any(|x| x == "src:humanizer"))));
}

// Two stores open on the same file each wrote back the snapshot they took when
// they opened, so whichever flushed last erased the other's work.
#[test]
fn a_second_store_does_not_erase_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut a = OverrideStore::open(&path).unwrap();
    let mut b = OverrideStore::open(&path).unwrap();

    a.set_disabled_tags(vec!["src:humanizer".into()]).unwrap();
    b.upsert_spelling_override(&SpellingRule::new(
        "視頻",
        vec!["影片".into()],
        RuleType::CrossStrait,
    ))
    .unwrap();

    let reopened = OverrideStore::open(&path).unwrap();
    assert_eq!(reopened.disabled_tags(), ["src:humanizer"]);
    assert!(
        reopened.overrides.spelling.iter().any(|r| r.from == "視頻"),
        "second writer's rule lost"
    );
}

// The disabled-tag union is applied to the merged result, so a pack can retire
// a family the base ruleset supplied. The test above exercises only the
// overrides layer and passes no active packs, which left this half of the union
// unexercised.
#[test]
fn a_pack_can_retire_a_base_rule_family() {
    let dir = tempfile::tempdir().unwrap();
    let base = crate::rules::loader::load_embedded_ruleset()
        .unwrap()
        .spelling_rules;
    let tagged = base
        .iter()
        .filter(|r| {
            !r.disabled
                && r.tags
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|x| x == "src:humanizer"))
        })
        .count();
    assert!(tagged > 1, "fixture needs a family, got {tagged}");

    let pack_dir = dir.path().join("packs");
    std::fs::create_dir_all(&pack_dir).unwrap();
    let pack = Overrides {
        disabled_tags: vec!["src:humanizer".into()],
        ..Default::default()
    };
    std::fs::write(
        pack_dir.join("retire.json"),
        serde_json::to_string(&pack).unwrap(),
    )
    .unwrap();

    let store = OverrideStore::open(&dir.path().join("o.json")).unwrap();
    let packs = PackStore::new(pack_dir);
    let (before, _) = build_merged_rules(&base, &[], &store, &packs, &[]);
    let (after, _) = build_merged_rules(&base, &[], &store, &packs, &["retire".to_string()]);
    assert_eq!(before.len() - after.len(), tagged);
    assert!(!after.iter().any(|r| r
        .tags
        .as_ref()
        .is_some_and(|t| t.iter().any(|x| x == "src:humanizer"))));
}

#[test]
fn colon_reveal_rule_remains_overridable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();
    let base = crate::rules::loader::load_embedded_ruleset()
        .unwrap()
        .spelling_rules;

    // Must be a rule that is enabled in the shipped ruleset, or the merge drops
    // it for being disabled and the assertion below passes whether or not the
    // override mechanism works.
    assert!(
        base.iter()
            .any(|rule| rule.from == "更可怕的是：" && !rule.disabled),
        "fixture rule must ship enabled for this test to prove anything"
    );
    store.disable_spelling_rule("更可怕的是：").unwrap();

    let merged = store.load_spelling_rules(&base);
    assert!(
        !merged.iter().any(|rule| rule.from == "更可怕的是："),
        "a disabled colon-reveal rule must not reach the scanner"
    );
}

#[test]
fn disable_builtin_case_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();
    let base = sample_base_case();

    assert_eq!(store.load_case_rules(&base).len(), 1);

    store.disable_case_rule("JavaScript").unwrap();
    assert_eq!(store.load_case_rules(&base).len(), 0);

    store.delete_case_override("JavaScript").unwrap();
    assert_eq!(store.load_case_rules(&base).len(), 1);
}

#[test]
fn case_override_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();
    let base = sample_base_case();

    let new_case = CaseRule {
        term: "TypeScript".into(),
        alternatives: None,
        disabled: false,
    };
    store.upsert_case_override(&new_case).unwrap();
    assert_eq!(store.load_case_rules(&base).len(), 2);

    let deleted = store.delete_case_override("TypeScript").unwrap();
    assert!(deleted);
    assert_eq!(store.load_case_rules(&base).len(), 1);
}

#[test]
fn overrides_persist_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");

    // Write an override.
    {
        let mut store = OverrideStore::open(&path).unwrap();
        let rule = SpellingRule::new("視頻", vec!["影片".into()], RuleType::CrossStrait);
        store.upsert_spelling_override(&rule).unwrap();
    }

    // Re-open and verify.
    let store = OverrideStore::open(&path).unwrap();
    let base = sample_base_spelling();
    let rules = store.load_spelling_rules(&base);
    assert_eq!(rules.len(), 3);
    assert!(rules.iter().any(|r| r.from == "視頻"));
}

#[test]
fn schema_version_mismatch_resets_and_backs_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");

    // Write overrides with wrong schema version.
    let bad = Overrides {
        schema_version: 1,
        metadata: None,
        spelling: vec![SpellingRule::new(
            "test",
            vec!["ok".into()],
            RuleType::CrossStrait,
        )],
        case: vec![],
        disabled_tags: Vec::new(),
    };
    std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();

    let store = OverrideStore::open(&path).unwrap();
    // Should have reset to empty.
    assert!(store.overrides.spelling.is_empty());
    assert_eq!(store.overrides.schema_version, SCHEMA_VERSION);

    // Old file should be backed up.
    let backup = dir.path().join("overrides.v1.bak");
    assert!(backup.exists(), "backup file should exist");
}

#[test]
fn corrupt_json_resets_and_backs_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");

    std::fs::write(&path, "{ this is not valid json }").unwrap();

    let store = OverrideStore::open(&path).unwrap();
    assert!(store.overrides.spelling.is_empty());
    assert_eq!(store.overrides.schema_version, SCHEMA_VERSION);

    let backup = dir.path().join("overrides.corrupt.bak");
    assert!(backup.exists(), "corrupt backup should exist");
}

#[test]
fn clear_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.json");
    let mut store = OverrideStore::open(&path).unwrap();

    store.disable_spelling_rule("test").unwrap();
    assert!(!store.overrides.spelling.is_empty());

    store.clear_overrides().unwrap();
    assert!(store.overrides.spelling.is_empty());
    assert!(store.overrides.case.is_empty());
}

// SuppressionStore tests

#[test]
fn suppression_add_remove() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suppressions.json");
    let mut store = SuppressionStore::open(&path).unwrap();

    assert!(store.list().is_empty());

    assert!(store.add("軟件").unwrap());
    assert!(!store.add("軟件").unwrap()); // duplicate
    assert!(store.is_suppressed("軟件"));
    assert!(!store.is_suppressed("硬件"));
    assert_eq!(store.list().len(), 1);

    assert!(store.remove("軟件").unwrap());
    assert!(!store.remove("軟件").unwrap()); // already removed
    assert!(!store.is_suppressed("軟件"));
}

#[test]
fn suppression_persists_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suppressions.json");

    {
        let mut store = SuppressionStore::open(&path).unwrap();
        store.add("信息").unwrap();
        store.add("網絡").unwrap();
    }

    let store = SuppressionStore::open(&path).unwrap();
    assert_eq!(store.list().len(), 2);
    assert!(store.is_suppressed("信息"));
    assert!(store.is_suppressed("網絡"));
}

#[test]
fn suppression_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suppressions.json");
    let mut store = SuppressionStore::open(&path).unwrap();

    store.add("test1").unwrap();
    store.add("test2").unwrap();
    assert_eq!(store.list().len(), 2);

    store.clear().unwrap();
    assert!(store.list().is_empty());
}

#[test]
fn suppression_default_file_created_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suppressions.json");
    let _store = SuppressionStore::open(&path).unwrap();
    assert!(path.exists());
}

#[test]
fn suppression_deduplicates_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suppressions.json");

    // Simulate a hand-edited file with duplicate terms.
    let duped = Suppressions {
        schema_version: SCHEMA_VERSION,
        terms: vec!["軟件".into(), "軟件".into(), "網絡".into()],
    };
    std::fs::write(&path, serde_json::to_string(&duped).unwrap()).unwrap();

    let mut store = SuppressionStore::open(&path).unwrap();
    assert_eq!(store.list().len(), 2);
    assert!(store.is_suppressed("軟件"));

    // After removing, both Vec and HashSet should agree.
    assert!(store.remove("軟件").unwrap());
    assert!(!store.is_suppressed("軟件"));
    assert_eq!(store.list().len(), 1);
}

// PackStore name validation tests

#[test]
fn pack_name_rejects_path_traversal() {
    assert!(PackStore::validate_pack_name("../evil").is_err());
    assert!(PackStore::validate_pack_name("foo/bar").is_err());
    assert!(PackStore::validate_pack_name("foo\\bar").is_err());
    assert!(PackStore::validate_pack_name("..").is_err());
    assert!(PackStore::validate_pack_name(".").is_err());
    assert!(PackStore::validate_pack_name("").is_err());
}

#[test]
fn pack_name_accepts_valid_names() {
    assert!(PackStore::validate_pack_name("medical").is_ok());
    assert!(PackStore::validate_pack_name("it-terms").is_ok());
    assert!(PackStore::validate_pack_name("my_pack.v2").is_ok());
}

// TranslationMemoryStore tests

#[test]
fn tm_record_and_suppress() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");
    let mut store = TranslationMemoryStore::open(&path).unwrap();

    assert!(store.list().is_empty());

    // Record a rejection (user kept the flagged term).
    store
        .record(TmEntry {
            found: "線程".into(),
            scanner_suggested: "執行緒".into(),
            user_chose: "線程".into(),
            context: Some("作業系統".into()),
            timestamp: "2026-03-18".into(),
        })
        .unwrap();

    assert_eq!(store.list().len(), 1);
    // Rejection suppresses the term regardless of context.
    assert!(store.should_suppress("線程"));
    // Different term is not suppressed.
    assert!(!store.should_suppress("調用"));
}

#[test]
fn tm_acceptance_does_not_suppress() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");
    let mut store = TranslationMemoryStore::open(&path).unwrap();

    // User accepted the scanner suggestion.
    store
        .record(TmEntry {
            found: "調用".into(),
            scanner_suggested: "呼叫".into(),
            user_chose: "呼叫".into(),
            context: None,
            timestamp: "2026-03-18".into(),
        })
        .unwrap();

    // Acceptance does not suppress (user_chose != found).
    assert!(!store.should_suppress("調用"));
}

#[test]
fn tm_deduplicates_by_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");
    let mut store = TranslationMemoryStore::open(&path).unwrap();

    // Record a rejection with context.
    store
        .record(TmEntry {
            found: "線程".into(),
            scanner_suggested: "執行緒".into(),
            user_chose: "線程".into(),
            context: Some("OS".into()),
            timestamp: "2026-03-18".into(),
        })
        .unwrap();
    assert!(store.should_suppress("線程"));

    // Accept with different context: overwrites the rejection (dedup by found).
    store
        .record(TmEntry {
            found: "線程".into(),
            scanner_suggested: "執行緒".into(),
            user_chose: "執行緒".into(),
            context: None,
            timestamp: "2026-03-19".into(),
        })
        .unwrap();

    assert_eq!(store.list().len(), 1);
    assert_eq!(store.list()[0].user_chose, "執行緒");
    // Acceptance overwrote rejection: no longer suppresses.
    assert!(!store.should_suppress("線程"));
}

#[test]
fn tm_persists_across_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");

    {
        let mut store = TranslationMemoryStore::open(&path).unwrap();
        store
            .record(TmEntry {
                found: "信息".into(),
                scanner_suggested: "資訊".into(),
                user_chose: "信息".into(),
                context: None,
                timestamp: "2026-03-18".into(),
            })
            .unwrap();
    }

    let store = TranslationMemoryStore::open(&path).unwrap();
    assert_eq!(store.list().len(), 1);
    assert!(store.should_suppress("信息"));
}

#[test]
fn tm_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");
    let mut store = TranslationMemoryStore::open(&path).unwrap();

    store
        .record(TmEntry {
            found: "test".into(),
            scanner_suggested: "ok".into(),
            user_chose: "test".into(),
            context: None,
            timestamp: "2026-03-18".into(),
        })
        .unwrap();
    assert_eq!(store.list().len(), 1);

    store.clear().unwrap();
    assert!(store.list().is_empty());
    assert!(!store.should_suppress("test"));
}

#[test]
fn tm_export_import_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("source.json");
    let dest_path = dir.path().join("dest.json");

    // Create source TM with entries.
    {
        let mut store = TranslationMemoryStore::open(&src_path).unwrap();
        store
            .record(TmEntry {
                found: "線程".into(),
                scanner_suggested: "執行緒".into(),
                user_chose: "執行緒".into(),
                context: None,
                timestamp: "2026-03-18".into(),
            })
            .unwrap();
        store
            .record(TmEntry {
                found: "內存".into(),
                scanner_suggested: "記憶體".into(),
                user_chose: "內存".into(),
                context: Some("casual".into()),
                timestamp: "2026-03-18".into(),
            })
            .unwrap();
        store.export(&dest_path).unwrap();
    }

    // Import into a fresh TM.
    let import_path = dir.path().join("target.json");
    let mut target = TranslationMemoryStore::open(&import_path).unwrap();
    let (added, updated) = target.import(&dest_path).unwrap();
    assert_eq!(added, 2);
    assert_eq!(updated, 0);
    assert_eq!(target.list().len(), 2);

    // Import again: updates existing, adds none.
    let (added2, updated2) = target.import(&dest_path).unwrap();
    assert_eq!(added2, 0);
    assert_eq!(updated2, 2);
    assert_eq!(target.list().len(), 2);
}

#[test]
fn tm_schema_mismatch_resets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");

    let bad = TranslationMemory {
        schema_version: 999,
        entries: vec![TmEntry {
            found: "test".into(),
            scanner_suggested: "ok".into(),
            user_chose: "test".into(),
            context: None,
            timestamp: "2026-03-18".into(),
        }],
    };
    std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();

    let store = TranslationMemoryStore::open(&path).unwrap();
    assert!(store.list().is_empty());

    let backup = dir.path().join(".zhtw-tm.v999.bak");
    assert!(backup.exists());
}

#[test]
fn tm_discover_path_at_git_root() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let sub = repo.join("src").join("deep");
    std::fs::create_dir_all(&sub).unwrap();

    let discovered = discover_tm_path(&sub);
    assert_eq!(discovered, repo.join(".zhtw-tm.json"));
}

#[test]
fn tm_hand_edited_duplicates_record_updates_last() {
    // Simulate a hand-edited TM file with duplicate found entries. record()
    // should update the last one (via index), and should_suppress() should read
    // the canonical one.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".zhtw-tm.json");

    let duped = TranslationMemory {
        schema_version: TM_SCHEMA_VERSION,
        entries: vec![
            TmEntry {
                found: "線程".into(),
                scanner_suggested: "執行緒".into(),
                user_chose: "線程".into(), // rejection
                context: None,
                timestamp: "2026-03-01".into(),
            },
            TmEntry {
                found: "線程".into(),
                scanner_suggested: "執行緒".into(),
                user_chose: "線程".into(), // also rejection (stale dup)
                context: Some("old".into()),
                timestamp: "2026-03-02".into(),
            },
        ],
    };
    std::fs::write(&path, serde_json::to_string(&duped).unwrap()).unwrap();

    let mut store = TranslationMemoryStore::open(&path).unwrap();
    assert!(store.should_suppress("線程")); // last entry is rejection

    // Now accept via record: should update the LAST entry.
    store
        .record(TmEntry {
            found: "線程".into(),
            scanner_suggested: "執行緒".into(),
            user_chose: "執行緒".into(), // acceptance
            context: None,
            timestamp: "2026-03-19".into(),
        })
        .unwrap();

    // should_suppress reads last entry, which is now acceptance.
    assert!(!store.should_suppress("線程"));
    // First (stale) entry is unchanged; record updated the last one.
    assert_eq!(store.list()[0].user_chose, "線程");
    assert_eq!(store.list()[1].user_chose, "執行緒");
}
