// MCP Resources: expose reference data as read-only MCP resources.
//
// Two resources:
//   zh-tw://style-guide/moe     MoE punctuation, variant, and vocabulary standards
//   zh-tw://dictionary/ambiguous   terms needing LLM disambiguation

use std::sync::OnceLock;

use rmcp::model::{ListResourcesResult, ReadResourceResult, Resource, ResourceContents};

use crate::rules::ruleset::SpellingRule;

/// URI for the MoE style guide resource.
pub const STYLE_GUIDE_URI: &str = "zh-tw://style-guide/moe";

/// URI for the ambiguous dictionary resource.
pub const AMBIGUOUS_DICT_URI: &str = "zh-tw://dictionary/ambiguous";

/// Return the list of available resources.
pub fn list_resources() -> ListResourcesResult {
    let style_guide = Resource::new(STYLE_GUIDE_URI, "MoE zh-TW Style Guide")
        .with_description("Ministry of Education punctuation, character variant, and vocabulary standards for Traditional Chinese (Taiwan)")
        .with_mime_type("text/markdown");
    let dictionary = Resource::new(AMBIGUOUS_DICT_URI, "Ambiguous Terms Dictionary")
        .with_description("Cross-strait terms requiring disambiguation, each entry with CN form, TW form, English anchor, and context")
        .with_mime_type("application/json");

    ListResourcesResult::with_all_items(vec![style_guide, dictionary])
}

/// Read a specific resource by URI.
///
/// The dictionary is a pure function of the ruleset, which is fixed once the
/// scanner is built, so `dict_cache` (owned by the server, not a global, since
/// one process can host servers with different rulesets) holds the rendered
/// JSON after the first read. Rebuilding it per read cost 322 KB of
/// pretty-printing while the server lock was held.
pub fn read_resource(
    uri: &str,
    spelling_rules: &[SpellingRule],
    dict_cache: &OnceLock<String>,
) -> Option<ReadResourceResult> {
    match uri {
        STYLE_GUIDE_URI => Some(read_style_guide()),
        AMBIGUOUS_DICT_URI => Some(read_ambiguous_dict(
            dict_cache.get_or_init(|| build_ambiguous_dict(spelling_rules)),
        )),
        _ => None,
    }
}

/// Generate the MoE style guide resource content.
fn read_style_guide() -> ReadResourceResult {
    let content = r#"# 教育部國語文標準 — zh-TW Style Guide

## Punctuation (重訂標點符號手冊)

| Mark | Half-width | Full-width (MoE) | Rule |
|------|-----------|-------------------|------|
| Comma | `,` | `，` U+FF0C | Always full-width in CJK prose |
| Period | `.` | `。` U+3002 | Hollow circle, not solid dot |
| Colon | `:` | `：` U+FF1A | Exception: UI string contexts may use half-width |
| Semicolon | `;` | `；` U+FF1B | Always full-width in CJK prose |
| Exclamation | `!` | `！` U+FF01 | Always full-width in CJK prose |
| Question | `?` | `？` U+FF1F | Always full-width in CJK prose |
| Enum comma | — | `、` U+3001 | For coordinate lists only (not clauses) |
| Primary quote | `"` `"` | `「` `」` | U+300C / U+300D |
| Secondary quote | | `『` `』` | U+300E / U+300F (nested only) |
| Book title | | `《` `》` | U+300A / U+300B |
| Ellipsis | `...` | `……` | Two U+2026 characters |
| Em dash | `--` | `──` | Two U+2500 or U+2014 |

## Character Variants (國字標準字體)

MoE standard forms differ from Kangxi, Hong Kong, and generic zh-Hant:

| Non-standard | MoE standard | Notes |
|-------------|-------------|-------|
| 裏 | 裡 | "inside" |
| 綫 | 線 | "thread/line" |
| 麪 | 麵 | "noodle" |
| 着 | 著 | particle (exception: chess 下著, proper nouns) |
| 台 | 臺 | Only in specific phrases: 臺灣, 臺北, 臺中, 臺南, 臺東, 臺大 |

Exceptions that keep `台`: 平台, 月台, 舞台, 台詞, 台階, 櫃台

## Vocabulary (Cross-strait Divergence)

Use Taiwan-standard vocabulary rather than Mainland China equivalents:

| CN term | TW term | English | Notes |
|---------|---------|---------|-------|
| 信息 | 資訊 | Information | 信息 in TW = "message" |
| 軟件 | 軟體 | Software | |
| 硬件 | 硬體 | Hardware | |
| 網絡 | 網路 | Network | |
| 默認 | 預設 | Default | 默認 in TW = "tacit approval" |
| 打印 | 列印 | Print | |
| 質量 | 品質 | Quality | 質量 in TW physics = "mass" |
| 視頻 | 影片 | Video | |
| 屏幕 | 螢幕 | Screen | |
| 程序 | 程式 | Program | 程序 in TW = "procedure" |
| 鼠標 | 滑鼠 | Mouse | |
| 接口 | 介面 | Interface | 接口 in TW = physical port (連接埠) |

## Profiles

- `base`: vocabulary + basic punctuation (default)
- `strict`: full MoE enforcement — all punctuation + character variants + 臺 normalization

## Capability Flags

- `relaxed`: for software UI — disables colon/dunhao/grammar enforcement, uses en-dash for ranges
- `detect_ai`: AI writing review — filler phrases, semantic safety words, density patterns
"#;

    text_resource(STYLE_GUIDE_URI, "text/markdown", content.into())
}

/// Generate the ambiguous dictionary from spelling rules that have an english
/// field.
fn read_ambiguous_dict(json: &str) -> ReadResourceResult {
    text_resource(AMBIGUOUS_DICT_URI, "application/json", json.to_owned())
}

/// One text resource, which is the only shape either resource here takes.
fn text_resource(uri: &str, mime_type: &str, text: String) -> ReadResourceResult {
    ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type(mime_type)
    ])
}

/// Render the ambiguous-terms dictionary. Called once per server.
fn build_ambiguous_dict(spelling_rules: &[SpellingRule]) -> String {
    let entries: Vec<serde_json::Value> = spelling_rules
        .iter()
        .filter(|r| r.english.is_some() && !r.disabled)
        .map(|r| {
            serde_json::json!({
                "from": r.from,
                "to": r.to,
                "english": r.english,
                "context": r.context,
                "rule_type": r.rule_type,
            })
        })
        .collect();

    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
#[path = "../../tests/unit/mcp/resources/tests.rs"]
mod tests;
