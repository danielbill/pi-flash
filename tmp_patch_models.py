import io

p = r'crates/app/src/settings/models.rs'
s = io.open(p, encoding='utf-8').read()
n0 = len(s)

# provider ids: catalog + creds (keep creds merge = TypeSafe fix)
s = s.replace("""    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.mc_display_catalog() {""",
"""    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.catalog_for(&self.cwd) {""")

s = s.replace("""    /// 设置页参照系的完整目录：会话目录（-ne 隔离）∪ 探测会话目录（带
    /// 扩展，含插件注册的 provider 如 pi-freeflow）。去重保持先见顺序。
    pub(crate) fn mc_display_catalog(&self) -> Vec<pi_link::protocol::ModelInfo> {
        let mut out: Vec<pi_link::protocol::ModelInfo> = self.catalog_for(&self.cwd).to_vec();
        for m in &self.mc_models_full {
            let r = format!("{}/{}", m.provider, m.id);
            if !out.iter().any(|e| format!("{}/{}", e.provider, e.id) == r) {
                out.push(m.clone());
            }
        }
        out
    }

    /// (enabled, total)""", """    /// (enabled, total)""")

s = s.replace("""        let catalog = self.mc_display_catalog();
        let models: Vec<&pi_link::protocol::ModelInfo> =
            catalog.iter().filter(|m| m.provider == provider).collect();""",
"""        let models: Vec<&pi_link::protocol::ModelInfo> = self
            .catalog_for(&self.cwd)
            .iter()
            .filter(|m| m.provider == provider)
            .collect();""")

s = s.replace("""        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
        // 设置页参照系（完整目录）同步重算；探测未回来时回落会话目录
        let refs = self.mc_display_refs();
        self.mc_display_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &refs);
    }

    /// 设置页参照系的 refs（完整目录；mc_refs 是 -ne 会话目录，供会话默认
    /// 模型选择使用——主会话没加载插件，显示与切换必须分开）。
    fn mc_display_refs(&self) -> Vec<String> {
        self.mc_display_catalog()
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }

    /// Initial model + thinking level for a NEW session (pi""",
"""        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
    }

    /// Initial model + thinking level for a NEW session (pi""")

s = s.replace("""        self.mc_patterns = edit.patterns;
        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
        let refs = self.mc_display_refs();
        self.mc_display_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &refs);
        cx.notify();
    }""",
"""        self.mc_patterns = edit.patterns;
        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
        cx.notify();
    }""")

i = s.index("        // 完整目录探测（插件注册的 provider）；一次性，插件装完（op 泵走到")
j = s.index("        // subagent profiles + agents settings")
k = s.index("    }", s.index("self.sa_profiles = pi_link::subagents::list_profiles"))
# cut from the probe comment line up to and including the probe fn, keep the reload tail
tail_anchor = "        self.sa_profiles = pi_link::subagents::list_profiles(&self.cwd, &agent_dir, &self.sa_settings);\n    }"
head = s[:i]
# find the end of the probe fn: the reload tail exists inside the replaced region? The probe
# comment sits AFTER the reload tail (reload ends with the profiles line + }). So:
# head keeps everything before the probe comment (which already contains the reload tail),
# and we drop from the comment through the end of mc_probe_full_catalog.
# The probe fn ends right before the next top-level item — find "    /// Skills toggle" marker:
m = s.index("    /// Skills toggle")
s = head + s[m:]

s = s.replace("""        let refs = self.mc_display_refs();
        match models_config::set_models_enabled(self.mc_patterns.as_ref(), &refs, &[r], enable) {""",
"""        match models_config::set_models_enabled(self.mc_patterns.as_ref(), &self.mc_refs(), &[r], enable) {""")
s = s.replace("""        let refs = self.mc_display_refs();
        match models_config::set_provider_enabled(self.mc_patterns.as_ref(), &refs, provider, enable) {""",
"""        match models_config::set_provider_enabled(self.mc_patterns.as_ref(), &self.mc_refs(), provider, enable) {""")
s = s.replace("""        let stale: Vec<String> = self.mc_display_state.stale.clone();""",
"""        let stale: Vec<String> = self.mc_state.stale.clone();""")

s = s.replace("""    let total_available = chat.mc_display_catalog().len();
    let enabled_total = if chat.mc_display_state.all_enabled {
        total_available
    } else {
        chat.mc_display_state.enabled.len()
    };
    let scoped = !chat.mc_display_state.all_enabled;
    let stale = chat.mc_display_state.stale.len();""",
"""    let total_available = chat.catalog_for(&chat.cwd).len();
    let enabled_total = if chat.mc_state.all_enabled {
        total_available
    } else {
        chat.mc_state.enabled.len()
    };
    let scoped = !chat.mc_state.all_enabled;
    let stale = chat.mc_state.stale.len();""")
s = s.replace("""    let enabled_set: std::collections::HashSet<String> =
        chat.mc_display_state.enabled.iter().cloned().collect();""",
"""    let enabled_set: std::collections::HashSet<String> =
        chat.mc_state.enabled.iter().cloned().collect();""")
s = s.replace("""                .children((!chat.mc_display_state.all_enabled).then(|| {""",
"""                .children((!chat.mc_state.all_enabled).then(|| {""")
s = s.replace("""    let all_enabled = chat.mc_display_state.all_enabled;
    let project_scope = chat.mc_project_scope;
    let last_guard = enabled_count == 1;
    let enabled_rows = enabled_set.clone();
    let pins = chat.mc_display_state.pins.clone();""",
"""    let all_enabled = chat.mc_state.all_enabled;
    let project_scope = chat.mc_project_scope;
    let last_guard = enabled_count == 1;
    let enabled_rows = enabled_set.clone();
    let pins = chat.mc_state.pins.clone();""")
s = s.replace("""    let merged = chat.mc_display_catalog();
    let models: Vec<pi_link::protocol::ModelInfo> = merged
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();""",
"""    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .catalog_for(&chat.cwd)
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();""")

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("models revert ok:", n0, "->", len(s))
assert "mc_display" not in s, "display leftovers!"
assert "mc_models_full" not in s, "models_full leftovers!"
assert "probe" not in s, "probe leftovers!"
print("clean")
恶