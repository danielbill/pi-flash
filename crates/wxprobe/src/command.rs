//! 命令解析（P2 切片 1）：`/帮助` 这类文本 → 结构化 `BotCommand`。
//!
//! 逐行对齐 ZCode `commandParser.ts`，含中英别名与「0 = 取消选择」语义。
//! 解析层只做文本 → 枚举，不碰任何 IO；语义映射（task → pi session 等）
//! 留到 P3 的 `pipeline.rs`。

/// 对齐 ZCode `shared/bots.ts:238-262` 的 `BotCommand`。
/// 命名保持 ZCode 原样（`Task*` 而非 `Session*`），才能和 TS 侧做逐字节对拍；
/// P3 再把 `Task*` 映射到 pi 的会话操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotCommand {
    Bind { code: String },
    Help,
    Status,
    New,
    Reconnect,
    WorkspaceList,
    WorkspaceSet { value: String },
    ModelList,
    ModelProviderSet { value: String },
    ModelSet { value: String },
    ModeList,
    ModeSet { value: String },
    ThoughtLevelList,
    ThoughtLevelSet { value: String },
    TaskList,
    TaskSet { value: String },
    ReplyList,
    ReplySet { value: String },
    Stop,
    PermissionRespond { value: String },
    ElicitationRespond { value: String },
    ElicitationSubmit,
    Approve { request_id: String, option_id: String },
    Deny { request_id: String },
    Unknown { name: String, raw: String },
    SelectionCancel,
    Message { text: String },
}

/// help / 数字菜单的展示顺序（对齐 `commandOrder.ts` 的 `BOT_MENU_COMMAND_ORDER`）。
/// P3 的 `/帮助` 渲染才消费；先标注避免 bin 构建报 dead code。
#[allow(dead_code)]
pub const BOT_MENU_ORDER: [&str; 9] = [
    "help",
    "status",
    "new",
    "workspace",
    "model",
    "mode",
    "thoughtLevel",
    "reply",
    "bind",
];

/// ZCode `splitCommand`：先 trim 判定 `/`，命令名按首个空白切开并**整段小写**，
/// 参数部分再 trim。注意参数本身不转小写（`provider `/`model ` 前缀是大小写敏感匹配）。
fn split_command(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let body = &trimmed[1..];
    let first_space = body
        .char_indices()
        .find(|(_, c)| c.is_whitespace());
    match first_space {
        None => Some((body.to_lowercase(), String::new())),
        Some((ix, c)) => Some((
            body[..ix].to_lowercase(),
            body[ix + c.len_utf8()..].trim().to_string(),
        )),
    }
}

/// ZCode `parseBotCommand`。
pub fn parse_bot_command(text: &str) -> BotCommand {
    let Some((name, rest)) = split_command(text) else {
        // 没有 `/` 前缀：`0` 是选择菜单的取消约定，其余原文交回给模型。
        // 注意这里 `text` 是**原始未 trim** 的（ZCode 同语义）。
        return if text.trim() == "0" {
            BotCommand::SelectionCancel
        } else {
            BotCommand::Message {
                text: text.to_string(),
            }
        };
    };

    let unknown = || BotCommand::Unknown {
        name: name.clone(),
        raw: text.to_string(),
    };
    // 结构体变体（命名字段）不是 fn 指针，用宏替代 wrapper 函数。
    // `list_or_set!`：ZCode 对 workspace/mode/thoughtLevel/task/reply 的
    // 「缺参数回 list、带参数回 set」语义；`with_rest!`：缺参数落 unknown
    // （permission / deny / bind 才是这个语义）。
    macro_rules! list_or_set {
        ($list:expr, $build:expr) => {
            if rest.is_empty() {
                $list
            } else {
                $build
            }
        };
    }
    macro_rules! with_rest {
        ($build:expr) => {
            if rest.is_empty() {
                unknown()
            } else {
                $build
            }
        };
    }

    match name.as_str() {
        "bind" => {
            if rest.is_empty() {
                unknown()
            } else {
                BotCommand::Bind {
                    code: rest.clone(),
                }
            }
        }
        "help" | "帮助" => BotCommand::Help,
        "cancel" | "取消" => BotCommand::SelectionCancel,
        "status" | "状态" => BotCommand::Status,
        "new" | "clear" | "新建" => BotCommand::New,
        "reconnect" | "重连" => BotCommand::Reconnect,
        "workspace" | "project" | "项目" => {
            list_or_set!(
                BotCommand::WorkspaceList,
                BotCommand::WorkspaceSet { value: rest.clone() }
            )
        }
        "model" | "模型" => {
            if rest.is_empty() {
                BotCommand::ModelList
            } else if let Some(v) = rest.strip_prefix("provider ") {
                BotCommand::ModelProviderSet {
                    value: v.trim().to_string(),
                }
            } else if let Some(v) = rest.strip_prefix("model ") {
                BotCommand::ModelSet {
                    value: v.trim().to_string(),
                }
            } else {
                BotCommand::ModelSet { value: rest }
            }
        }
        "mode" | "模式" => {
            list_or_set!(
                BotCommand::ModeList,
                BotCommand::ModeSet { value: rest.clone() }
            )
        }
        "thoughtlevel" | "thought_level" | "thought-level" | "think" | "思考" => {
            list_or_set!(
                BotCommand::ThoughtLevelList,
                BotCommand::ThoughtLevelSet { value: rest.clone() }
            )
        }
        "task" => {
            list_or_set!(
                BotCommand::TaskList,
                BotCommand::TaskSet { value: rest.clone() }
            )
        }
        "reply" | "回复" => {
            list_or_set!(
                BotCommand::ReplyList,
                BotCommand::ReplySet { value: rest.clone() }
            )
        }
        "stop" | "停止" => BotCommand::Stop,
        "permission" => with_rest!(BotCommand::PermissionRespond { value: rest.clone() }),
        "elicitation" | "answer" | "回答" => {
            if rest.is_empty() {
                return unknown();
            }
            if ["submit", "done", "完成", "提交"].contains(&rest.to_lowercase().as_str()) {
                BotCommand::ElicitationSubmit
            } else {
                BotCommand::ElicitationRespond { value: rest }
            }
        }
        "approve" => {
            // ZCode `const [requestId, optionId] = rest.split(/\s+/)` ——
            // 只取前两个，**多余参数忽略**（`/approve a b c` 仍是 Approve{a,b}）。
            let mut parts = rest.split_whitespace();
            let request_id = parts.next();
            let option_id = parts.next();
            match (request_id, option_id) {
                (Some(request_id), Some(option_id)) => BotCommand::Approve {
                    request_id: request_id.to_string(),
                    option_id: option_id.to_string(),
                },
                _ => unknown(),
            }
        }
        "deny" => with_rest!(BotCommand::Deny { request_id: rest.clone() }),
        _ => unknown(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(input: &str) -> BotCommand {
        parse_bot_command(input)
    }

    #[test]
    fn english_aliases_match_zcode() {
        assert_eq!(cmd("/help"), BotCommand::Help);
        assert_eq!(cmd("/status"), BotCommand::Status);
        assert_eq!(cmd("/new"), BotCommand::New);
        assert_eq!(cmd("/clear"), BotCommand::New);
        assert_eq!(cmd("/reconnect"), BotCommand::Reconnect);
        assert_eq!(cmd("/stop"), BotCommand::Stop);
        assert_eq!(cmd("/cancel"), BotCommand::SelectionCancel);
    }

    #[test]
    fn chinese_aliases_match_zcode() {
        assert_eq!(cmd("/帮助"), BotCommand::Help);
        assert_eq!(cmd("/状态"), BotCommand::Status);
        assert_eq!(cmd("/新建"), BotCommand::New);
        assert_eq!(cmd("/重连"), BotCommand::Reconnect);
        assert_eq!(cmd("/停止"), BotCommand::Stop);
        assert_eq!(cmd("/取消"), BotCommand::SelectionCancel);
        assert_eq!(cmd("/回复"), BotCommand::ReplyList);
        assert_eq!(cmd("/思考"), BotCommand::ThoughtLevelList);
    }

    #[test]
    fn command_name_is_lowercased_but_argument_is_not() {
        // 命令名整段小写（ZCode `body.toLowerCase()`），参数保持原样 ——
        // `provider `/`model ` 是大小写敏感前缀。
        assert_eq!(cmd("/STATUS"), BotCommand::Status);
        assert_eq!(
            cmd("/MODEL provider OpenAI"),
            BotCommand::ModelProviderSet {
                value: "OpenAI".into()
            }
        );
        assert_eq!(
            cmd("/model model GPT-5"),
            BotCommand::ModelSet { value: "GPT-5".into() }
        );
    }

    #[test]
    fn model_command_three_shapes() {
        assert_eq!(cmd("/模型"), BotCommand::ModelList);
        assert_eq!(
            cmd("/模型 glm-5"),
            BotCommand::ModelSet { value: "glm-5".into() }
        );
        assert_eq!(
            cmd("/模型 provider zhipu"),
            BotCommand::ModelProviderSet {
                value: "zhipu".into()
            }
        );
    }

    #[test]
    fn list_when_argument_missing_set_when_present() {
        assert_eq!(cmd("/项目"), BotCommand::WorkspaceList);
        assert_eq!(
            cmd("/项目 pi-flash"),
            BotCommand::WorkspaceSet {
                value: "pi-flash".into()
            }
        );
        assert_eq!(cmd("/模式"), BotCommand::ModeList);
        assert_eq!(
            cmd("/模式 yolo"),
            BotCommand::ModeSet { value: "yolo".into() }
        );
        assert_eq!(cmd("/task"), BotCommand::TaskList);
        assert_eq!(
            cmd("/task abc"),
            BotCommand::TaskSet { value: "abc".into() }
        );
        assert_eq!(cmd("/回复"), BotCommand::ReplyList);
        assert_eq!(
            cmd("/回复 streaming_card"),
            BotCommand::ReplySet {
                value: "streaming_card".into()
            }
        );
    }

    #[test]
    fn plain_zero_is_selection_cancel() {
        assert_eq!(cmd("0"), BotCommand::SelectionCancel);
        assert_eq!(cmd("  0  "), BotCommand::SelectionCancel);
        assert_eq!(cmd("00"), BotCommand::Message { text: "00".into() });
    }

    #[test]
    fn non_command_text_is_passed_through_untrimmed() {
        // ZCode `{ type:"message", text }` 用的是**原始** text，不是 trim 过的。
        assert_eq!(
            cmd("  帮我看看这个 bug  "),
            BotCommand::Message {
                text: "  帮我看看这个 bug  ".into()
            }
        );
    }

    #[test]
    fn leading_and_trailing_whitespace_is_tolerated_for_commands() {
        assert_eq!(cmd("   /状态  "), BotCommand::Status);
        assert_eq!(cmd("\t/停止\n"), BotCommand::Stop);
    }

    #[test]
    fn approve_and_deny_take_exactly_one_or_two_ids() {
        assert_eq!(
            cmd("/approve req1 opt1"),
            BotCommand::Approve {
                request_id: "req1".into(),
                option_id: "opt1".into()
            }
        );
        assert!(
            matches!(cmd("/approve req1"), BotCommand::Unknown { .. }),
            "/approve 少一个参数必须落 unknown"
        );
        assert_eq!(
            cmd("/deny req1"),
            BotCommand::Deny {
                request_id: "req1".into()
            }
        );
        assert_eq!(cmd("/deny"), BotCommand::Unknown { name: "deny".into(), raw: "/deny".into() });
        // ZCode `const [requestId, optionId] = rest.split(/\s+/)` 只取前两个，多余参数忽略。
        assert_eq!(
            cmd("/approve req1 opt1 extra"),
            BotCommand::Approve {
                request_id: "req1".into(),
                option_id: "opt1".into()
            }
        );
    }

    #[test]
    fn elicitation_submit_keywords_are_case_insensitive() {
        assert_eq!(cmd("/回答 提交"), BotCommand::ElicitationSubmit);
        assert_eq!(cmd("/answer DONE"), BotCommand::ElicitationSubmit);
        assert_eq!(
            cmd("/回答 是的"),
            BotCommand::ElicitationRespond {
                value: "是的".into()
            }
        );
        assert_eq!(cmd("/回答"), BotCommand::Unknown { name: "回答".into(), raw: "/回答".into() });
    }

    #[test]
    fn bind_without_code_falls_to_unknown() {
        assert_eq!(cmd("/bind"), BotCommand::Unknown { name: "bind".into(), raw: "/bind".into() });
        assert_eq!(
            cmd("/bind ABC123"),
            BotCommand::Bind {
                code: "ABC123".into()
            }
        );
    }

    #[test]
    fn unknown_command_keeps_raw_input() {
        assert_eq!(
            cmd("/不存在的东西 x"),
            BotCommand::Unknown {
                name: "不存在的东西".into(),
                raw: "/不存在的东西 x".into()
            }
        );
        assert_eq!(
            cmd("/"),
            BotCommand::Unknown {
                name: String::new(),
                raw: "/".into()
            }
        );
    }

    #[test]
    fn permission_requires_argument() {
        assert_eq!(
            cmd("/permission 1"),
            BotCommand::PermissionRespond {
                value: "1".into()
            }
        );
        assert_eq!(
            cmd("/permission"),
            BotCommand::Unknown { name: "permission".into(), raw: "/permission".into() }
        );
    }

    #[test]
    fn menu_order_matches_zcode() {
        assert_eq!(
            BOT_MENU_ORDER,
            [
                "help",
                "status",
                "new",
                "workspace",
                "model",
                "mode",
                "thoughtLevel",
                "reply",
                "bind"
            ]
        );
    }
}
