use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn m(provider: &str, id: &str) -> pi_link::protocol::ModelInfo {
        pi_link::protocol::ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            provider: provider.to_string(),
            context_window: None,
            max_tokens: None,
            reasoning: None,
            input: None,
            cost: None,
        }
    }

    /// 候选下标 → 显示序下标（0 起全序）。
    fn order(models: &[pi_link::protocol::ModelInfo], default_ref: Option<&str>) -> Vec<usize> {
        let cand: Vec<usize> = (0..models.len()).collect();
        order_model_indices(models, &cand, default_ref)
    }

    #[test]
    fn default_model_group_goes_first() {
        let models = vec![
            m("openai", "gpt-5"),
            m("deepseek", "ds-v4"),
            m("openai", "o3"),
        ];
        // 默认 = deepseek/ds-v4 → 该商组置顶、组内置首
        let ord = order(&models, Some("deepseek/ds-v4"));
        assert_eq!(ord, vec![1, 0, 2]);
    }

    #[test]
    fn default_model_first_inside_group() {
        let models = vec![m("openai", "a"), m("openai", "b"), m("openai", "c")];
        let ord = order(&models, Some("openai/b"));
        assert_eq!(ord, vec![1, 0, 2]);
    }

    #[test]
    fn other_providers_sorted_by_display_name() {
        let models = vec![
            m("zai-coding-cn", "glm"),
            m("deepseek", "ds"),
            m("openai", "gpt"),
        ];
        // 无默认模型 → 按 Provider 显示名：DeepSeek / OpenAI / Z.AI Coding CN
        let ord = order(&models, None);
        assert_eq!(ord, vec![1, 2, 0]);
    }

    #[test]
    fn default_not_in_candidates_is_ignored() {
        let models = vec![m("openai", "gpt"), m("deepseek", "ds")];
        // 默认模型已被过滤掉 → 退回纯显示名序
        let ord = order(&models, Some("anthropic/claude"));
        assert_eq!(ord, vec![1, 0]);
    }

    #[test]
    fn empty_and_single_are_untouched() {
        let models: Vec<pi_link::protocol::ModelInfo> = vec![];
        assert!(order(&models, Some("openai/gpt")).is_empty());
        let models = vec![m("openai", "gpt")];
        assert_eq!(order(&models, Some("openai/gpt")), vec![0]);
    }
}
