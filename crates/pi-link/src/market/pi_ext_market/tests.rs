use super::*;

const SEARCH_FIXTURE: &str = r#"{
  "objects": [
    {"downloads":{"monthly":1802478},"updated":"2026-10-19T22:33:01.605Z",
     "package":{"name":"billion-context","description":"ctx compression","version":"0.1.202",
                "date":"2026-10-19T09:50:11.928Z",
                "links":{"repository":"git+https://github.com/ranxianglei/billion-context.git"}}},
    {"package":{"name":"@scope/pi-tool","version":"2.0.0"}}
  ],
  "total": 12364
}"#;

#[test]
fn search_url_encodes_and_weights_popularity() {
    let u = search_url("web search", 50, 50);
    assert!(u.contains("/-/v1/search?text="), "{u}");
    assert!(u.contains("keywords%3Api-package%20web%20search"), "{u}");
    assert!(u.contains("size=50") && u.contains("from=50"), "{u}");
    assert!(u.contains("popularity=1&quality=0&maintenance=0"), "{u}");
    // 空词只带关键词
    let u = search_url("   ", 0, 50);
    assert!(u.contains("text=keywords%3Api-package&"), "{u}");
}

#[test]
fn latest_url_escapes_scope_slash() {
    assert_eq!(
        latest_url("@scope/pkg"),
        format!("{}/@scope%2Fpkg/latest", crate::market::registry_base())
    );
    assert!(latest_url("plain").ends_with("/plain/latest"));
}

#[test]
fn parse_search_page_tolerates_missing_fields() {
    let page = parse_search_page(SEARCH_FIXTURE).unwrap();
    assert_eq!(page.total, 12364);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].monthly_downloads, 1_802_478);
    assert_eq!(
        page.entries[0].repo.as_deref(),
        Some("https://github.com/ranxianglei/billion-context")
    );
    assert!(page.entries[1].description.is_empty());
    assert_eq!(page.entries[1].monthly_downloads, 0);
    assert_eq!(page.entries[1].name, "@scope/pi-tool");
    assert!(parse_search_page("not json").is_err());
}

#[test]
fn parse_latest_manifest_maps_pi_keys() {
    let body = r#"{"name":"x","pi":{"extensions":["./dist"],"skills":["./s"],
                    "prompts":null,"themes":[]}}"#;
    assert_eq!(
        parse_latest_manifest(body).unwrap(),
        vec![MarketKind::Extension, MarketKind::Skill, MarketKind::Theme]
    );
    // 无 pi 键 = 空表（不是错误）
    assert!(parse_latest_manifest(r#"{"name":"x"}"#).unwrap().is_empty());
    assert!(parse_latest_manifest("{bad").is_err());
}

#[test]
fn is_extension_only_hides_known_non_extensions() {
    let mut e = MarketEntry {
        name: "a".into(),
        ..Default::default()
    };
    // 清单未补全 → 放行
    assert!(e.is_extension());
    e.kinds = vec![MarketKind::Skill];
    assert!(!e.is_extension());
    e.kinds = vec![MarketKind::Extension, MarketKind::Skill];
    assert!(e.is_extension());
}

#[test]
fn parse_author_and_days_since() {
    let body = r#"{"objects":[{"package":{"name":"x","publisher":{"username":"nicopreme"}}}]}"#;
    let page = parse_search_page(body).unwrap();
    assert_eq!(page.entries[0].author.as_deref(), Some("nicopreme"));
    // 2026-10-20T12:00:00Z 的秒数 = 1792497600
    assert_eq!(
        days_since("2026-10-19T22:33:01.605Z", 1_792_497_600),
        Some(1)
    );
    assert_eq!(days_since("2026-10-20T00:10:00Z", 1_792_497_600), Some(0));
    assert_eq!(days_since("1970-01-01", 0), Some(0));
    assert_eq!(days_since("", 0), None);
    assert_eq!(days_since("garbage", 0), None);
}

#[test]
fn install_command_and_source() {
    let e = MarketEntry {
        name: "@s/p".into(),
        ..Default::default()
    };
    assert_eq!(e.install_command(), "pi install npm:@s/p");
    assert_eq!(e.source(), "npm:@s/p");
}

#[test]
fn format_downloads_scales() {
    assert_eq!(format_downloads(999), "999");
    assert_eq!(format_downloads(3_400), "3.4K");
    assert_eq!(format_downloads(792_800), "792.8K");
    assert_eq!(format_downloads(1_800_000), "1.8M");
    assert_eq!(format_downloads(1_000_000), "1M");
}

#[test]
fn home_snapshot_roundtrip_and_bad_file() {
    let dir = std::env::temp_dir().join(format!("pf-mkt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snap.json");

    // 坏文件 → None（不 panic）
    std::fs::write(&path, "{oops").unwrap();
    assert!(HomeSnapshot::load_at(&path).is_none());

    let snap = HomeSnapshot::new_now(
        "web",
        vec![MarketEntry {
            name: "pi-web-access".into(),
            kinds: vec![MarketKind::Extension],
            ..Default::default()
        }],
    );
    snap.save_at(&path).unwrap();
    let back = HomeSnapshot::load_at(&path).unwrap();
    assert_eq!(back.query, "web");
    assert_eq!(back.entries.len(), 1);
    assert_eq!(back.entries[0].kinds, vec![MarketKind::Extension]);
    assert!(back.age_secs().abs() < 5);

    // 上限截断
    let many = HomeSnapshot::new_now("q", vec![MarketEntry::default(); 80]);
    many.save_at(&path).unwrap();
    assert_eq!(
        HomeSnapshot::load_at(&path).unwrap().entries.len(),
        SNAPSHOT_MAX
    );
    let _ = std::fs::remove_dir_all(&dir);
}
