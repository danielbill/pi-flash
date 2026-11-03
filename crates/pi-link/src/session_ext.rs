//! 会话级插件清单（031/034：full+plugins「自定义」档按会话保存一份）。
//!
//! 落 `~/.pi-flash/session-ext.json`（pi-flash 自有文件，不碰 pi 的会话文件）：
//!
//! ```json
//! { "<会话 key>": ["npm:pi-freeflow", "npm:pi-web-access"] }
//! ```
//!
//! key = `SessionRuntime::key`：真实会话 = 会话文件绝对路径；草稿 = `draft-N`
//! （草稿不跨重启，写进去也无害，重开应用后自然失效）。
//!
//! 只存「自定义清单」本身，不存工具预设 —— 预设重启回落默认档（自定义）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// 会话键归一化（Windows 上同一份会话文件有两种写法：app 的 `session.open`
/// 给正斜杠、pi 的 `get_state.sessionFile` 给反斜杠；盘符大小写也不固定）。
/// 台账两侧（读/写/删）都走这里，保证「自定义」清单跨重启对得上。
pub fn norm_key(key: &str) -> String {
    let s = key.trim().replace('\\', "/");
    let mut chars: Vec<char> = s.chars().collect();
    if chars.len() > 1 && chars[1] == ':' {
        chars[0] = chars[0].to_ascii_lowercase();
    }
    chars.into_iter().collect()
}

/// 读取整张表（文件缺失/损坏都当空表：清单丢了最坏只是回落到空选择）。
pub fn read(path: &Path) -> BTreeMap<String, Vec<String>> {
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return BTreeMap::new(),
    };
    let Ok(Value::Object(map)) = crate::config::parse_lenient(&text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| {
            let list: Vec<String> = v
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            Some((norm_key(&k), list))
        })
        .collect()
}

/// 某个会话的清单（空表/无此会话 → 空 vec）。
pub fn read_for(path: &Path, key: &str) -> Vec<String> {
    read(path).remove(&norm_key(key)).unwrap_or_default()
}

/// 写入某个会话的清单（空清单 = 删掉该条目，不落空数组）。
pub fn write_for(path: &Path, key: &str, sources: &[String]) -> Result<(), String> {
    let mut table = read(path);
    let key = norm_key(key);
    if sources.is_empty() {
        table.remove(&key);
    } else {
        table.insert(key, sources.to_vec());
    }
    write_all(path, &table)
}

/// 整表写盘（原子替换；台账小，直接整写）。
pub fn write_all(path: &Path, table: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    let value = Value::Object(
        table
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Value::Array(v.iter().map(|s| Value::String(s.clone())).collect()),
                )
            })
            .collect(),
    );
    crate::config::write_json(path, &value)
}

/// 清掉某个会话的条目（会话被删除时用）。
pub fn remove_for(path: &Path, key: &str) -> Result<(), String> {
    let mut table = read(path);
    if table.remove(&norm_key(key)).is_none() {
        return Ok(());
    }
    write_all(path, &table)
}

/// 路径型挂载条目（PF 插件 `pi_extensions` 绑定通道写入的绝对路径，契约 v3）。
/// 台账是**混合通道**：用户勾选的 scheme 源（`npm:`/`git:`…）+ 插件挂载的
/// 路径条目。勾选面板宇宙只含 scheme 源——路径条目不显示、不计数，但落盘
/// 时必须保住（丢 = 下轮 spawn 插件 ext 不挂）。
pub fn is_mount_path(src: &str) -> bool {
    std::path::Path::new(src).is_absolute()
}

/// 用户勾选清单 + 既有路径型条目合并（保序去重）：`user` 为基准，
/// `existing` 里的路径条目原样保留，scheme 条目以 `user` 为准。
pub fn with_mounts_preserved(existing: &[String], user: &[String]) -> Vec<String> {
    let mut out = user.to_vec();
    for src in existing {
        if is_mount_path(src) && !out.contains(src) {
            out.push(src.clone());
        }
    }
    out
}

/// [`with_mounts_preserved`] + 落盘一步到位（勾选面板/重绑共用的写入口）。
pub fn write_for_preserving_mounts(path: &Path, key: &str, user: &[String]) -> Result<(), String> {
    let existing = read_for(path, key);
    write_for(path, key, &with_mounts_preserved(&existing, user))
}

/// 单条挂载源自愈（090-10 版本层换代）：路径型条目的绝对路径里带着**版本
/// 目录**（`…/plugins/<pkg>/<ver>/pi/index.mjs`，market 布局），插件升级后
/// 旧版本目录被删，台账仍指旧路径 → spawn 把它当 `-e` 传给 pi，扩展加载失败
/// 会让**整个进程直接退出**（真机 2026-11-03：绑定微信后重启，会话起不来，
/// ext 无处安身，收发全哑）。这里按当前安装包重解析同名文件。
///
/// 返回 `None` = 插件包内那条路径已经没救了（文件改名/包已卸载）——调用方
/// 必须摘掉它：一条死路径毒死整批 `-e`，宁可少挂一条。非插件包路径原样返回
/// （用户自有的绝对路径条目，PF 不替它判生死）。
fn resolve_mount_source(src: &str) -> Option<String> {
    if std::path::Path::new(src).is_file() {
        return Some(src.to_string());
    }
    let norm = src.replace('\\', "/");
    let Some(i) = norm.rfind("/plugins/") else {
        return Some(src.to_string());
    };
    let rest = &norm[i + "/plugins/".len()..];
    let (pkg, rel) = rest.split_once('/')?;
    if pkg.is_empty() || rel.is_empty() {
        return None;
    }
    let pkg_dir = Path::new(&norm[..i + "/plugins/".len() + pkg.len()]).to_path_buf();
    // 候选相对路径：原样（平铺 / 版本未变）+ 去掉首段版本目录（换代）
    let mut cands: Vec<&str> = vec![rel];
    if let Some((head, tail)) = rel.split_once('/')
        && !tail.is_empty()
        && head.starts_with(|c: char| c.is_ascii_digit())
    {
        cands.push(tail);
    }
    // 候选根：当前清单所在目录（版本层 = 最新版本目录；平铺 = 包根）→ 包根
    // → 任一子目录（换代后清单还没落的残留）
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(cur) = crate::service_ext::pkg_manifest_path(&pkg_dir)
        .and_then(|m| m.parent().map(Path::to_path_buf))
    {
        roots.push(cur);
    }
    roots.push(pkg_dir.clone());
    if let Ok(rd) = std::fs::read_dir(&pkg_dir) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subs.sort();
        roots.extend(subs);
    }
    for root in &roots {
        for cand in &cands {
            let f = root.join(cand);
            if f.is_file() {
                return Some(f.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 两条扩展源是否指同一项（路径分隔符 / 盘符大小写不敏感）。台账里同一份
/// 路径可能落成 `C:\…\pi/index.mjs` 或 `c:/…/pi/index.mjs` 两种写法，spawn 拼
/// `-e`、attach 合并去重、自愈比对都必须过这道门，否则同一挂载会挂两遍
/// （ext 双开）或永远合并不上。
pub fn same_source(a: &str, b: &str) -> bool {
    norm_key(a).eq_ignore_ascii_case(&norm_key(b))
}

/// 整份会话清单自愈：路径型条目按当前安装包重解析、死条目摘除、重复条目
/// 去重（同一条 `-e` 挂两次 = ext 双开）。返回（新清单，是否变动）——
/// 调用方拿到 `true` 时**回写台账**，下一次 spawn 起就不再付解析成本。
pub fn resolve_sources(srcs: &[String]) -> (Vec<String>, bool) {
    let mut out: Vec<String> = Vec::with_capacity(srcs.len());
    let mut changed = false;
    for src in srcs {
        let resolved = if is_mount_path(src) {
            resolve_mount_source(src)
        } else {
            Some(src.clone())
        };
        let Some(r) = resolved else {
            changed = true; // 死条目：摘
            continue;
        };
        if out.iter().any(|x| same_source(x, &r)) {
            changed = true; // 重复条目：去重
            continue;
        }
        if !same_source(&r, src) {
            changed = true;
        }
        out.push(r);
    }
    (out, changed)
}

/// 便捷取路径（`~/.pi-flash/session-ext.json`）。
pub fn store_path() -> Option<PathBuf> {
    crate::paths::session_ext_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-flash-sessext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn roundtrip_per_conversation() {
        let p = tmp("roundtrip.json");
        let _ = std::fs::remove_file(&p);
        write_for(&p, "C:/s/a.jsonl", &["npm:x".into(), "npm:y".into()]).unwrap();
        write_for(&p, "C:/s/b.jsonl", &["npm:z".into()]).unwrap();
        assert_eq!(read_for(&p, "C:/s/a.jsonl"), vec!["npm:x", "npm:y"]);
        assert_eq!(read_for(&p, "C:/s/b.jsonl"), vec!["npm:z"]);
        assert!(read_for(&p, "missing").is_empty());
    }

    #[test]
    fn empty_list_removes_entry_and_missing_file_is_empty() {
        let p = tmp("empty.json");
        let _ = std::fs::remove_file(&p);
        assert!(read(&p).is_empty(), "文件缺失 = 空表");
        write_for(&p, "k", &["npm:a".into()]).unwrap();
        write_for(&p, "k", &[]).unwrap();
        assert!(read(&p).is_empty(), "空清单不落空数组");
        assert!(remove_for(&p, "nope").is_ok());
    }

    #[test]
    fn key_normalization_matches_both_windows_spellings() {
        let p = tmp("norm.json");
        let _ = std::fs::remove_file(&p);
        // 写入 pi 的反斜杠形态（get_state.sessionFile），读出用 app 的正斜杠形态
        let win = format!("C:\\Users\\bi_da\\.pi\\agent\\sessions\\x.jsonl");
        write_for(&p, &win, &["npm:a".into()]).unwrap();
        assert_eq!(
            read_for(&p, "C:/Users/bi_da/.pi/agent/sessions/x.jsonl"),
            vec!["npm:a"]
        );
        // 反向：写正斜杠、读出反斜杠 + 盘符小写
        write_for(&p, "C:/tmp/y.jsonl", &["npm:b".into()]).unwrap();
        let win2 = format!("c:\\tmp\\y.jsonl");
        assert_eq!(read_for(&p, &win2), vec!["npm:b"]);
    }

    /// 插件挂载通道（真机 2026-10-31：勾 2 显 [3]）——scheme 源以勾选为准，
    /// 路径型条目跨勾选保留；路径判定与平台绝对路径口径一致。
    #[test]
    fn mount_paths_survive_user_pick_rewrites() {
        let mount = std::env::temp_dir()
            .join("plugins/demo-plugin/pi/index.mjs")
            .to_string_lossy()
            .into_owned();
        assert!(is_mount_path(&mount), "绝对路径 = 挂载条目");
        assert!(!is_mount_path("npm:pi-freeflow"));
        assert!(!is_mount_path("git:https://github.com/u/r"));

        let existing = vec!["npm:old".into(), mount.clone()];
        // 用户取消勾了 npm:old、新勾 npm:new——old 该走就走，mount 必须留下
        let merged = with_mounts_preserved(&existing, &["npm:new".to_string()]);
        assert_eq!(merged, vec!["npm:new".to_string(), mount.clone()]);

        // 幂等：以合并结果为 existing 再合并不增生
        let again = with_mounts_preserved(&merged, &["npm:new".to_string()]);
        assert_eq!(again, merged);
    }

    #[test]
    fn write_preserving_mounts_roundtrip() {
        let p = tmp("preserve.json");
        let _ = std::fs::remove_file(&p);
        let mount = std::env::temp_dir()
            .join("plugins/demo-plugin/pi/index.mjs")
            .to_string_lossy()
            .into_owned();
        // 先按插件挂载通道的形态落一份（历史 attach 曾整表覆写：台账只剩路径）
        write_for(&p, "k", &[mount.clone()]).unwrap();
        // 用户勾选落盘：mount 保留，npm 勾选进来
        write_for_preserving_mounts(&p, "k", &["npm:pi-freeflow".into()]).unwrap();
        assert_eq!(
            read_for(&p, "k"),
            vec!["npm:pi-freeflow".to_string(), mount]
        );
    }

    #[test]
    fn broken_file_degrades_to_empty() {
        let p = tmp("broken.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(read(&p).is_empty());
    }
}

/// 迁移清理（90-6）：把某插件的执行域入口从**所有**会话的扩展清单里摘掉。
///
/// 契约 v4 起，插件 `pi/index.mjs`（以及清单 contributes.service 指的任何
/// `pi/*.mjs`）是执行域入口，不再是可 `-e` 挂载的 pi 扩展；v3 时代写下的
/// 会话清单还指着它 —— pi 在 spawn 时直接报
/// `does not export a valid factory function`，同批 `-e`（含执行域 shim）
/// 一起失效：插件功能面全哑（2026-10-30 真机根因）。
///
/// 按**插件目录**匹配而非整条路径相等：数据目录被搬迁/复制后旧清单里的
/// 绝对路径可能与当前插件目录不同，仍要摘掉。
///
/// 包目录名两形态都认 + 版本层容忍（090-10）：market 落
/// `…/plugins/@<scope>__<id>/<ver>/pi/…`，dev 平铺落 `…/plugins/<id>/pi/…`。
/// 此前 needle 写死 `/plugins/<id>/pi/`，对 market 包（scope 前缀 + 版本层）
/// 永不命中 → 停用摘不掉条目 → 再启用被「清单已含」短路、pi 不重启 →
/// ext 停在 unload 水位终态，通信死（真机 2026-11-02）。
/// 幂等：没有残留就不写盘。
pub fn purge_exec_entries(plugin_id: &str) -> Result<usize, String> {
    let Some(store) = store_path() else {
        return Ok(0);
    };
    if !store.is_file() {
        return Ok(0);
    }
    let id = plugin_id.to_ascii_lowercase();
    let mut table = read(&store);
    let mut removed = 0usize;
    for list in table.values_mut() {
        let before = list.len();
        list.retain(|p| !is_plugin_pi_entry(&norm_key(p).to_ascii_lowercase(), &id));
        removed += before - list.len();
    }
    if removed > 0 {
        table.retain(|_, v| !v.is_empty());
        write_all(&store, &table)?;
    }
    Ok(removed)
}

/// 路径是否指到**本插件**的 pi 入口：`…/plugins/<pkg>/[<ver>/]pi/…`，
/// 其中 `<pkg>` = `<id>`（dev 平铺）或 `@<scope>__<id>`（market 目录别名）。
fn is_plugin_pi_entry(path_norm_lower: &str, id_lower: &str) -> bool {
    let Some(i) = path_norm_lower.rfind("/plugins/") else {
        return false;
    };
    let rest = &path_norm_lower[i + "/plugins/".len()..];
    let mut seg = rest.split('/');
    let pkg = seg.next().unwrap_or("");
    if !(pkg == id_lower || pkg.ends_with(&format!("__{id_lower}"))) {
        return false;
    }
    // 之后是 `pi/…`（平铺）或 `<版本>/pi/…`（版本层，版本目录首字符为数字）
    let after: Vec<&str> = seg.collect();
    match after.len() {
        0 => false,
        1 => after[0] == "pi",
        _ => {
            after[0] == "pi"
                || (after[0].starts_with(|c: char| c.is_ascii_digit()) && after[1] == "pi")
        }
    }
}

/// 挂载源自愈单测（090-10 版本层换代；真机 2026-11-03 重启哑火根因）。
#[cfg(test)]
mod resolve_mount_tests {
    use super::*;

    fn sandbox() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-flash-mountheal-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plant(pkg_dir: &Path, ver: Option<&str>, rel: &str) -> PathBuf {
        let base = match ver {
            Some(v) => pkg_dir.join(v),
            None => pkg_dir.to_path_buf(),
        };
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("pf-plugin.toml"), "schema = 5\n").unwrap();
        let f = base.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "export function activate() {}").unwrap();
        f
    }

    /// 真机形态：台账里是旧版本目录的绝对路径，磁盘上只剩新版本目录。
    #[test]
    fn stale_version_dir_heals_to_current() {
        let root = sandbox();
        let pkg = root.join("plugins").join("@pi-flash-plugins__pf-wxbot");
        let live = plant(&pkg, Some("0.2.0"), "pi/index.mjs");
        // 旧版本目录已不在（市场安装换代时被清掉）
        let stale = pkg.join("0.1.4").join("pi/index.mjs");
        let srcs = vec![
            "npm:pi-freeflow".to_string(),
            stale.to_string_lossy().into_owned(),
        ];
        let (healed, changed) = resolve_sources(&srcs);
        assert!(changed, "版本层重定位必须报变动（调用方要回写台账）");
        assert_eq!(healed[0], "npm:pi-freeflow", "scheme 源原样保留");
        assert!(
            same_source(&healed[1], &live.to_string_lossy()),
            "指向当前版本目录"
        );
        assert!(Path::new(&healed[1]).is_file(), "自愈结果必须是活文件");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 平铺 dev 布局（无版本层）路径本来就对：不自愈、不报变动。
    #[test]
    fn flat_layout_is_left_alone() {
        let root = sandbox();
        let pkg = root.join("plugins").join("pf-wxbot");
        let live = plant(&pkg, None, "pi/index.mjs");
        let srcs = vec![live.to_string_lossy().into_owned()];
        let (healed, changed) = resolve_sources(&srcs);
        assert!(!changed);
        assert_eq!(healed, srcs);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 包内文件改名/包已卸载：条目没救了 → 摘掉（绝不能把死路径喂给 pi，
    /// 一条死 `-e` 会让 pi 整个退出）。
    #[test]
    fn dead_plugin_entry_is_dropped() {
        let root = sandbox();
        let pkg = root.join("plugins").join("@x__gone");
        plant(&pkg, Some("0.2.0"), "pi/index.mjs");
        let srcs = vec![
            pkg.join("0.1.0")
                .join("pi/old-entry.mjs")
                .to_string_lossy()
                .into_owned(),
        ];
        let (healed, changed) = resolve_sources(&srcs);
        assert!(changed);
        assert!(healed.is_empty(), "死条目必须摘除");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 非插件包的用户自有绝对路径：PF 不替它判生死（原样留着）。
    #[test]
    fn foreign_absolute_path_is_preserved() {
        let root = sandbox();
        let other = root.join("my-ext").join("index.mjs");
        let srcs = vec![other.to_string_lossy().into_owned()];
        let (healed, changed) = resolve_sources(&srcs);
        assert!(!changed);
        assert_eq!(healed, srcs);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同一文件两条写法（旧版本 + 新版本）：去重成一条（-e 挂两次 = ext 双开）。
    #[test]
    fn duplicates_collapse() {
        let root = sandbox();
        let pkg = root.join("plugins").join("@x__p");
        let live = plant(&pkg, Some("0.2.0"), "pi/index.mjs");
        let srcs = vec![
            live.to_string_lossy().into_owned(),
            pkg.join("0.1.4")
                .join("pi/index.mjs")
                .to_string_lossy()
                .into_owned(),
        ];
        let (healed, changed) = resolve_sources(&srcs);
        assert!(changed);
        assert_eq!(healed.len(), 1);
        assert!(same_source(&healed[0], &live.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod purge_exec_entry_tests {
    use super::*;

    #[test]
    fn matches_market_versioned_dev_flat_and_rejects_others() {
        let id = "pf-wxbot";
        // market：@scope__id + 版本层
        assert!(is_plugin_pi_entry(
            "c:/r/plugins/@pi-flash-plugins__pf-wxbot/0.1.3/pi/index.mjs",
            id
        ));
        // dev 平铺：<id>/pi/
        assert!(is_plugin_pi_entry("c:/r/plugins/pf-wxbot/pi/index.mjs", id));
        // 别的插件（__ 前缀不同于 id）：不匹配
        assert!(!is_plugin_pi_entry(
            "c:/r/plugins/@x__wxbotpro/0.1.0/pi/index.mjs",
            id
        ));
        // 本插件但非 pi 入口（data/svc 通道文件）：不匹配
        assert!(!is_plugin_pi_entry(
            "c:/r/plugins/@pi-flash-plugins__pf-wxbot/0.1.3/data/svc/in",
            id
        ));
        // 无 /plugins/ 段：不匹配
        assert!(!is_plugin_pi_entry("npm:pi-freeflow", id));
    }
}
