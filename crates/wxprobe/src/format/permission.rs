//! ZCode `shared/permission-request-preview.ts`（366 行）的移植。
//!
//! 与 PC 端 `ui/PermissionDialog.tsx` **共享同一份投影** —— 微信里的
//! 「需要权限：写入中 src/app.ts」和桌面弹窗的标题走的是同一套解析，
//! 这是「界面对齐」在权限这条线上的落点。
//!
//! 实现差异：ZCode 用 `seen: Set<unknown>` 做身份去重，那是为 JS 对象图
//! （可循环引用）准备的；JSON 输入是树，但 `Object.values()` 会**重复走进**
//! 已经被显式处理过的 `rawInput/input/params/toolCall`，没有 `seen` 会指数级
//! 重入。这里保留等价物：用 `*const Value` 做节点身份（树内节点地址唯一）。

use std::collections::HashSet;

use serde_json::Value;

use super::summary::normalize_display_text;

/// ZCode `MAX_PERMISSION_FILE_PATHS = 6`。
pub const MAX_PERMISSION_FILE_PATHS: usize = 6;

const COMMAND_KEYS: [&str; 4] = ["command", "cmd", "script", "shellcommand"];
const ARGUMENT_KEYS: [&str; 3] = ["args", "argv", "arguments"];
/// ZCode 注释：`file_path` / `filePath` 漏掉会让权限预览只剩标题。
const FILE_PATH_KEYS: [&str; 12] = [
    "path",
    "paths",
    "file",
    "file_path",
    "filepath",
    "files",
    "filename",
    "filenames",
    "target",
    "targets",
    "location",
    "locations",
];
const IGNORED_DIRECTORY_KEYS: [&str; 3] = ["cwd", "directory", "workingdirectory"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Command,
    File,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Add,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionFileChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequestPreview {
    pub title: String,
    pub command: Option<String>,
    pub file_paths: Vec<String>,
    pub scope: Scope,
    pub file_change: Option<PermissionFileChange>,
    pub file_changes: Vec<PermissionFileChange>,
}

/// 对应 ZCode `ZCodePermissionRequest` 中预览用到的四个字段
/// （`title?` / `description` / `kind` / `raw`）。
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub title: Option<String>,
    pub description: String,
    pub kind: String,
    pub raw: Value,
}

type Seen<'a> = HashSet<*const Value>;

fn normalize_block_text(value: &str) -> String {
    value.trim().replace("\r\n", "\n")
}

/// ZCode `getStringArray`：字符串 trim、数字/布尔转字符串，其余丢弃。
fn get_string_array(value: &Value) -> Vec<String> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| match item {
            Value::String(s) => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// ZCode `readPermissionInputSource`。
///
/// 注意 `"rawInput" in rawSource && ... !== undefined`：JSON 里键存在即为真
/// （哪怕值是 `null`），所以判据是 **键是否存在**，不是值是否为 null。
fn read_permission_input_source(raw: &Value) -> &Value {
    let Some(record) = raw.as_object() else {
        return raw;
    };
    if record.contains_key("rawInput") {
        return &record["rawInput"];
    }
    if record.contains_key("input") {
        return &record["input"];
    }
    raw
}

/// ZCode `getCommandFromRecord`：按**对象键插入序**找第一个命令键，
/// 再按插入序找参数键拼接。依赖 `serde_json` 的 `preserve_order`
/// （由 `pi-link` 开启，feature 全局统一）——去掉它这里会退化成字典序。
fn get_command_from_record(record: &serde_json::Map<String, Value>) -> Option<String> {
    for (key, value) in record {
        let lowered = key.to_lowercase();
        if !COMMAND_KEYS.contains(&lowered.as_str()) {
            continue;
        }
        let Value::String(text) = value else { continue };
        let command = normalize_block_text(text);
        if command.is_empty() {
            continue;
        }
        for (args_key, args_value) in record {
            if !ARGUMENT_KEYS.contains(&args_key.to_lowercase().as_str()) {
                continue;
            }
            let args = get_string_array(args_value);
            if !args.is_empty() {
                return Some(format!("{command} {}", args.join(" ")));
            }
        }
        return Some(command);
    }
    None
}

const NESTED_COMMAND_KEYS: [&str; 4] = ["rawInput", "input", "params", "toolCall"];

/// ZCode `findFirstCommand`。
fn find_first_command(value: &Value, seen: &mut Seen<'_>, allow_bare_string: bool) -> Option<String> {
    if let Some(text) = value.as_str() {
        if !allow_bare_string {
            return None;
        }
        let command = normalize_block_text(text);
        return (!command.is_empty()).then_some(command);
    }
    if let Some(list) = value.as_array() {
        let ptr = value as *const Value;
        if !seen.insert(ptr) {
            return None;
        }
        for item in list {
            if let Some(found) = find_first_command(item, seen, false) {
                return Some(found);
            }
        }
        return None;
    }
    let Some(record) = value.as_object() else {
        return None;
    };
    let ptr = value as *const Value;
    if !seen.insert(ptr) {
        return None;
    }
    if let Some(direct) = get_command_from_record(record) {
        return Some(direct);
    }
    for key in NESTED_COMMAND_KEYS {
        let Some(nested) = record.get(key) else {
            continue;
        };
        // 只有前三个是 allowBare —— `toolCall` 那一路不接受裸字符串。
        let allow = key != "toolCall";
        if let Some(found) = find_first_command(nested, seen, allow) {
            return Some(found);
        }
    }
    for nested in record.values() {
        if !nested.is_array() && !nested.is_object() {
            continue;
        }
        if let Some(found) = find_first_command(nested, seen, false) {
            return Some(found);
        }
    }
    None
}

fn push_unique_path(paths: &mut Vec<String>, value: &str) {
    let normalized = normalize_display_text(value);
    if normalized.is_empty() || paths.contains(&normalized) {
        return;
    }
    paths.push(normalized);
}

/// ZCode `extractPathsFromCandidate`：字符串直接收；对象先收 `path` 再遍历所有值。
fn extract_paths_from_candidate(value: &Value, paths: &mut Vec<String>, seen: &mut Seen<'_>) {
    if paths.len() >= MAX_PERMISSION_FILE_PATHS {
        return;
    }
    if let Some(text) = value.as_str() {
        push_unique_path(paths, text);
        return;
    }
    if let Some(list) = value.as_array() {
        let ptr = value as *const Value;
        if !seen.insert(ptr) {
            return;
        }
        for item in list {
            extract_paths_from_candidate(item, paths, seen);
            if paths.len() >= MAX_PERMISSION_FILE_PATHS {
                return;
            }
        }
        return;
    }
    let Some(record) = value.as_object() else {
        return;
    };
    let ptr = value as *const Value;
    if !seen.insert(ptr) {
        return;
    }
    if let Some(path) = record.get("path").and_then(Value::as_str) {
        push_unique_path(paths, path);
        if paths.len() >= MAX_PERMISSION_FILE_PATHS {
            return;
        }
    }
    for nested in record.values() {
        extract_paths_from_candidate(nested, paths, seen);
        if paths.len() >= MAX_PERMISSION_FILE_PATHS {
            return;
        }
    }
}

/// ZCode `collectFilePaths`：按键名分派，`cwd/directory/workingdirectory` 明确忽略。
fn collect_file_paths(value: &Value, paths: &mut Vec<String>, seen: &mut Seen<'_>) {
    if paths.len() >= MAX_PERMISSION_FILE_PATHS {
        return;
    }
    if let Some(list) = value.as_array() {
        let ptr = value as *const Value;
        if !seen.insert(ptr) {
            return;
        }
        for item in list {
            collect_file_paths(item, paths, seen);
            if paths.len() >= MAX_PERMISSION_FILE_PATHS {
                return;
            }
        }
        return;
    }
    let Some(record) = value.as_object() else {
        return;
    };
    let ptr = value as *const Value;
    if !seen.insert(ptr) {
        return;
    }
    for (key, candidate) in record {
        let lowered = key.to_lowercase();
        if IGNORED_DIRECTORY_KEYS.contains(&lowered.as_str()) {
            continue;
        }
        if FILE_PATH_KEYS.contains(&lowered.as_str()) {
            extract_paths_from_candidate(candidate, paths, seen);
            if paths.len() >= MAX_PERMISSION_FILE_PATHS {
                return;
            }
            continue;
        }
        if !candidate.is_array() && !candidate.is_object() {
            continue;
        }
        collect_file_paths(candidate, paths, seen);
        if paths.len() >= MAX_PERMISSION_FILE_PATHS {
            return;
        }
    }
}

/// ZCode `collectFileChanges`：只认 `{ path: { type: "add"|"update" } }` 这个形状。
fn collect_file_changes(value: &Value, changes: &mut Vec<PermissionFileChange>, seen: &mut Seen<'_>) {
    if let Some(list) = value.as_array() {
        let ptr = value as *const Value;
        if !seen.insert(ptr) {
            return;
        }
        for item in list {
            collect_file_changes(item, changes, seen);
        }
        return;
    }
    let Some(record) = value.as_object() else {
        return;
    };
    let ptr = value as *const Value;
    if !seen.insert(ptr) {
        return;
    }
    if let Some(map) = record.get("changes").and_then(Value::as_object) {
        for (path, change) in map {
            let Some(kind) = change.get("type").and_then(Value::as_str) else {
                continue;
            };
            let kind = match kind {
                "add" => ChangeKind::Add,
                "update" => ChangeKind::Update,
                _ => continue,
            };
            if changes.iter().any(|c| c.path == *path && c.kind == kind) {
                continue;
            }
            changes.push(PermissionFileChange {
                path: path.clone(),
                kind,
            });
        }
    }
    for nested in record.values() {
        if !nested.is_array() && !nested.is_object() {
            continue;
        }
        collect_file_changes(nested, changes, seen);
    }
}

/// ZCode `getPermissionRequestPreview`（`permission-request-preview.ts:338`）。
pub fn get_permission_request_preview(request: &PermissionRequest) -> PermissionRequestPreview {
    let raw = &request.raw;
    let mut file_paths = Vec::new();
    collect_file_paths(raw, &mut file_paths, &mut Seen::new());

    let title = request
        .title
        .as_deref()
        .map(normalize_display_text)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let d = normalize_display_text(&request.description);
            (!d.is_empty()).then_some(d)
        })
        .or_else(|| {
            let k = normalize_display_text(&request.kind);
            (!k.is_empty()).then_some(k)
        })
        .unwrap_or_else(|| "permission".to_string());

    let command = find_first_command(read_permission_input_source(raw), &mut Seen::new(), true);

    let mut file_changes = Vec::new();
    collect_file_changes(raw, &mut file_changes, &mut Seen::new());

    let file_change = (file_changes.len() == 1).then(|| file_changes[0].clone());
    let scope = if command.is_some() {
        Scope::Command
    } else if !file_paths.is_empty() {
        Scope::File
    } else {
        Scope::Generic
    };

    PermissionRequestPreview {
        title,
        command,
        file_paths,
        scope,
        file_change,
        file_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preview(raw: Value) -> PermissionRequestPreview {
        get_permission_request_preview(&PermissionRequest {
            title: None,
            description: "desc".into(),
            kind: "bash".into(),
            raw,
        })
    }

    #[test]
    fn title_falls_back_description_then_kind_then_permission() {
        let p = get_permission_request_preview(&PermissionRequest {
            title: Some("  标题  ".into()),
            description: "desc".into(),
            kind: "bash".into(),
            raw: json!({}),
        });
        assert_eq!(p.title, "标题", "title 归一空白");

        let p = get_permission_request_preview(&PermissionRequest {
            title: None,
            description: "  描述  ".into(),
            kind: "bash".into(),
            raw: json!({}),
        });
        assert_eq!(p.title, "描述");

        let p = get_permission_request_preview(&PermissionRequest {
            title: None,
            description: "   ".into(),
            kind: "bash".into(),
            raw: json!({}),
        });
        assert_eq!(p.title, "bash", "description 为空白时落到 kind");

        let p = get_permission_request_preview(&PermissionRequest {
            title: None,
            description: "".into(),
            kind: "".into(),
            raw: json!({}),
        });
        assert_eq!(p.title, "permission");
    }

    #[test]
    fn command_reads_raw_input_then_input_then_nested_params() {
        // rawInput 优先（`!== undefined` = 键存在即取）
        let p = preview(json!({
            "rawInput": { "command": "ls", "args": ["-la"] },
            "input": { "command": "echo nope" }
        }));
        assert_eq!(p.command.as_deref(), Some("ls -la"));

        // 只有 input
        let p = preview(json!({ "input": { "command": "ls" } }));
        assert_eq!(p.command.as_deref(), Some("ls"));

        // 键存在但值是 null：JS `!== undefined` 为真 → 取到 null → 无命令
        let p = preview(json!({ "input": null, "other": { "command": "ls" } }));
        assert_eq!(p.command, None, "input 键在场就不该继续往别处找命令");

        // 深层 params
        let p = preview(json!({ "params": { "nested": { "cmd": "pwd" } } }));
        assert_eq!(p.command.as_deref(), Some("pwd"));
    }

    #[test]
    fn bare_string_only_allowed_from_explicit_source() {
        // 顶层字符串（raw 本身是字符串）→ allowBare=true → 收
        let p = preview(json!("cargo build"));
        assert_eq!(p.command.as_deref(), Some("cargo build"));
        // 普通嵌套里的裸字符串 → allowBare=false → 不收
        let p = preview(json!({ "x": "cargo build" }));
        assert_eq!(p.command, None);
    }

    #[test]
    fn file_paths_dedup_cap_and_ignore_cwd() {
        let p = preview(json!({
            "path": "/a/one.rs",
            "paths": ["/a/one.rs", "/a/two.rs"],
            "cwd": "/should/be/ignored",
            "directory": "/also/ignored",
            "file_path": "/a/three.rs"
        }));
        assert_eq!(p.file_paths, vec!["/a/one.rs", "/a/two.rs", "/a/three.rs"]);
        assert_eq!(p.scope, Scope::File);

        // 上限 6
        let many: Vec<String> = (0..10).map(|i| format!("/f{i}.rs")).collect();
        let p = preview(json!({ "files": many }));
        assert_eq!(p.file_paths.len(), MAX_PERMISSION_FILE_PATHS);
    }

    #[test]
    fn scope_priority_is_command_then_file_then_generic() {
        assert_eq!(preview(json!({ "command": "ls" })).scope, Scope::Command);
        assert_eq!(preview(json!({ "path": "/a" })).scope, Scope::File);
        assert_eq!(preview(json!({ "other": 1 })).scope, Scope::Generic);
    }

    #[test]
    fn file_changes_only_from_changes_map() {
        let p = preview(json!({
            "changes": { "src/a.rs": { "type": "add" }, "src/b.rs": { "type": "update" } }
        }));
        assert_eq!(p.file_changes.len(), 2);
        assert_eq!(p.file_changes[0].kind, ChangeKind::Add);
        assert!(p.file_change.is_none(), "多于一条时 fileChange 为 null");

        let p = preview(json!({ "changes": { "src/a.rs": { "type": "add" } } }));
        assert_eq!(p.file_change.as_ref().map(|c| c.path.as_str()), Some("src/a.rs"));

        // 非法 type 被忽略
        let p = preview(json!({ "changes": { "x.rs": { "type": "delete" } } }));
        assert!(p.file_changes.is_empty());
    }

    #[test]
    fn recursion_does_not_blow_up_on_revisited_nodes() {
        // `input` 会被显式处理一遍、再被通用 `Object.values` 撞上一次；
        // 没有身份去重，深链遍历是 2^d（32 层 = 42 亿次）。有 seen 才是线性的。
        let mut node = json!({});
        for _ in 0..32 {
            node = json!({ "input": node });
        }
        let p = preview(node);
        assert_eq!(p.command, None);
    }
}
