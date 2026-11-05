//! 幻影绑定守卫 + 绑定对账（伪标签前置）：pi 在 SessionManager 构造期
//! 就报 sessionFile，文件本体要等首条 user/assistant 消息才落盘
//! （_hasConversation/_persist，#10000）。没落盘不得绑定——boot 草稿保
//! 持 draft 身份、recents/last-open 不记幻影路径；落盘了才迁移。

use super::*;
use gpui::TestAppContext;

fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
    cx.new(|_| {
        let mut r = SessionRuntime::new("draft-t".into(), PathBuf::from("C:/t"), None);
        r.sound_on = false;
        r
    })
}

/// follow_session_file 存在性守卫：幻影路径不绑定，已落盘文件正常绑定。
#[gpui::test]
fn follow_requires_existing_file(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.follow_session_file(PathBuf::from("Z:/pf-not-landed/x.jsonl"), cx);
        assert!(r.file.is_none(), "幻影路径不得绑定");
    });
    let real = std::env::temp_dir().join(format!("pf-guard-{}.jsonl", std::process::id()));
    std::fs::write(&real, "{}\n").unwrap();
    rt.update(cx, |r, cx| {
        r.follow_session_file(real.clone(), cx);
        assert_eq!(
            r.file.as_deref(),
            Some(real.as_path()),
            "已落盘文件正常绑定"
        );
    });
    let _ = std::fs::remove_file(&real);
}

/// 绑定对账三态：state 空 → GetState 兜底返回 true；报过未落盘 →
/// false（调用方撤伪标签）；已落盘 → 绑定 + true。
#[gpui::test]
fn try_bind_reported_file_states(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        assert!(
            r.try_bind_reported_file(cx),
            "state 空 = 绑定在途，伪标签先留着"
        );
        assert!(r.file.is_none());
    });
    rt.update(cx, |r, cx| {
        r.state = Some(SessionState {
            session_file: Some("Z:/pf-not-landed/x.jsonl".into()),
            ..Default::default()
        });
        assert!(
            !r.try_bind_reported_file(cx),
            "未落盘的报备文件不得视为已绑定"
        );
        assert!(r.file.is_none());
    });
    let real = std::env::temp_dir().join(format!("pf-guard-bind-{}.jsonl", std::process::id()));
    std::fs::write(&real, "{}\n").unwrap();
    rt.update(cx, |r, cx| {
        r.state = Some(SessionState {
            session_file: Some(real.to_string_lossy().into_owned()),
            ..Default::default()
        });
        assert!(r.try_bind_reported_file(cx));
        assert_eq!(r.file.as_deref(), Some(real.as_path()));
    });
    let _ = std::fs::remove_file(&real);
}
