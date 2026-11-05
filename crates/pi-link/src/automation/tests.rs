use super::*;
use crate::json::parse_value;

fn parse(line: &str) -> Option<AutomationRecord> {
    // 走和线上一致的深 JSON 安全解析器
    let v: Value = parse_value(line).ok()?;
    parse_line(&serde_json::to_string(&v).ok()?)
}

#[test]
fn hello_auth_roundtrip() {
    let h = hello_record(PROTO_VERSION, "pi-flash", 4812, "0.1.1");
    assert_eq!(
        parse(&h.to_string()),
        Some(AutomationRecord::Hello {
            proto: 1,
            app: "pi-flash".into(),
            pid: 4812,
            version: "0.1.1".into(),
        })
    );
    let a = auth_record("tok");
    assert_eq!(
        parse(&a.to_string()),
        Some(AutomationRecord::Auth {
            token: "tok".into()
        })
    );
    assert_eq!(
        parse(&auth_ok_record().to_string()),
        Some(AutomationRecord::AuthOk)
    );
    let ae = auth_error_record("unauthorized", "bad token");
    assert_eq!(
        parse(&ae.to_string()),
        Some(AutomationRecord::AuthError {
            code: "unauthorized".into(),
            message: "bad token".into(),
        })
    );
}

#[test]
fn request_response_roundtrip() {
    let req = request_record("cli-1", method::UI_SNAPSHOT, &json!({"surface": "files"}));
    assert_eq!(
        parse(&req.to_string()),
        Some(AutomationRecord::Request {
            id: "cli-1".into(),
            method: "ui.snapshot".into(),
            params: json!({"surface": "files"}),
        })
    );
    // params 缺省 = {}
    let bare = json!({"id": "r2", "method": "app.info"});
    assert_eq!(
        parse(&bare.to_string()),
        Some(AutomationRecord::Request {
            id: "r2".into(),
            method: "app.info".into(),
            params: json!({}),
        })
    );
    let ok = response_ok_record("cli-1", json!({"cwd": "/w"}));
    assert_eq!(
        parse(&ok.to_string()),
        Some(AutomationRecord::Response {
            id: "cli-1".into(),
            ok: true,
            result: Some(json!({"cwd": "/w"})),
            error: None,
        })
    );
    let err = response_error_record("cli-1", &AutomationError::new("unknown_method", "nope"));
    assert_eq!(
        parse(&err.to_string()),
        Some(AutomationRecord::Response {
            id: "cli-1".into(),
            ok: false,
            result: None,
            error: Some(AutomationError {
                code: "unknown_method".into(),
                message: "nope".into()
            }),
        })
    );
}

#[test]
fn garbage_lines_are_ignored() {
    assert_eq!(parse(""), None);
    assert_eq!(parse("   \n"), None);
    assert_eq!(parse("not json at all"), None);
    assert_eq!(parse("\u{1b}[2Jclear"), None);
}

#[test]
fn unknown_records_are_kept() {
    // 未知 type → Unknown（不静默丢，排查线协议漂移靠它）
    let v = json!({"type": "from_the_future", "x": 1});
    assert_eq!(parse(&v.to_string()), Some(AutomationRecord::Unknown(v)));
    // 无 id 无 type 的 JSON 也归 Unknown
    let v = json!({"foo": 1});
    assert_eq!(parse(&v.to_string()), Some(AutomationRecord::Unknown(v)));
}

#[test]
fn instance_file_roundtrip_and_order() {
    let base = std::env::temp_dir().join(format!("pif-auto-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let a = InstanceInfo {
        pid: 11,
        port: 5001,
        token: "ta".into(),
        started_at_ms: 100,
    };
    let b = InstanceInfo {
        pid: 22,
        port: 5002,
        token: "tb".into(),
        started_at_ms: 200,
    };
    write_instance_file_in(&base, &a).unwrap();
    write_instance_file_in(&base, &b).unwrap();
    let got = read_instances_in(&base);
    assert_eq!(got.len(), 2);
    // 新的在前
    assert_eq!(got[0].0, b);
    assert_eq!(got[1].0, a);
    // 坏文件跳过不 panic
    std::fs::write(instance_file_in(&base, 33), "garbage{").unwrap();
    assert_eq!(read_instances_in(&base).len(), 2);
    let _ = std::fs::remove_dir_all(&base);
}
