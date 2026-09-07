use super::*;
use tempfile::TempDir;

#[test]
fn discover_finds_config_in_cwd() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join(CONFIG_FILENAME),
        "profile = \"strict\"\nmax_errors = 5\n",
    )
    .unwrap();
    let cfg = ProjectConfig::discover(dir.path()).unwrap();
    assert_eq!(cfg.profile.as_deref(), Some("strict"));
    assert_eq!(cfg.max_errors, Some(5));
}

#[test]
fn discover_walks_upward() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub").join("deep");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(dir.path().join(CONFIG_FILENAME), "profile = \"base\"\n").unwrap();
    let cfg = ProjectConfig::discover(&sub).unwrap();
    assert_eq!(cfg.profile.as_deref(), Some("base"));
}

#[test]
fn discover_stops_at_git_root() {
    let dir = TempDir::new().unwrap();
    // Place config above .git boundary.
    std::fs::write(dir.path().join(CONFIG_FILENAME), "profile = \"base\"\n").unwrap();
    let sub = dir.path().join("repo");
    std::fs::create_dir_all(sub.join(".git")).unwrap();
    let deep = sub.join("src");
    std::fs::create_dir_all(&deep).unwrap();
    // Discovery from deep should not find the config above .git.
    let cfg = ProjectConfig::discover(&deep);
    assert!(cfg.is_none());
}

#[test]
fn discover_returns_none_when_absent() {
    let dir = TempDir::new().unwrap();
    // Create .git so we stop quickly.
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    assert!(ProjectConfig::discover(dir.path()).is_none());
}

#[test]
fn parse_all_fields() {
    let toml = r#"
profile = "strict"
content_type = "markdown"
max_errors = 0
max_warnings = 10
ignore_terms = ["軟件", "硬件"]
exclude = ["vendor/**", "*.tmp"]
overrides = "/path/to/overrides.json"
suppressions = "/path/to/suppressions.json"
packs = ["medical", "legal"]
"#;
    let cfg: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.profile.as_deref(), Some("strict"));
    assert_eq!(cfg.content_type.as_deref(), Some("markdown"));
    assert_eq!(cfg.max_errors, Some(0));
    assert_eq!(cfg.max_warnings, Some(10));
    assert_eq!(cfg.ignore_terms.as_ref().unwrap().len(), 2);
    assert_eq!(cfg.exclude.as_ref().unwrap().len(), 2);
    assert_eq!(cfg.overrides.as_deref(), Some("/path/to/overrides.json"));
    assert_eq!(cfg.packs.as_ref().unwrap(), &["medical", "legal"]);
}

#[test]
fn parse_empty_config() {
    let cfg: ProjectConfig = toml::from_str("").unwrap();
    assert!(cfg.profile.is_none());
    assert!(cfg.max_errors.is_none());
}
