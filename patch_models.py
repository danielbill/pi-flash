import io

p = r'crates/pi-link/src/protocol.rs'
s = io.open(p, encoding='utf-8').read()

if 'GetAvailableModels' in s:
    print('already present')
else:
    s = s.replace(
        "    SetSessionName { name: String },\n"
        "    GetCommands,\n"
        "}",
        "    SetSessionName { name: String },\n"
        "    GetCommands,\n"
        "    GetAvailableModels,\n"
        "    SetThinkingLevel { level: String },\n"
        "}"
    )
    s = s.replace(
        "            Command::SetSessionName { .. } => \"set_session_name\",\n"
        "            Command::GetCommands => \"get_commands\",",
        "            Command::SetSessionName { .. } => \"set_session_name\",\n"
        "            Command::GetCommands => \"get_commands\",\n"
        "            Command::GetAvailableModels => \"get_available_models\",\n"
        "            Command::SetThinkingLevel { .. } => \"set_thinking_level\","
    )
    s = s.replace(
        "            Command::Abort\n"
        "            | Command::GetState\n"
        "            | Command::GetMessages\n"
        "            | Command::GetSessionStats\n"
        "            | Command::GetCommands => {\n"
        "                json!({ \"type\": self.kind() })\n"
        "            }",
        "            Command::Abort\n"
        "            | Command::GetState\n"
        "            | Command::GetMessages\n"
        "            | Command::GetSessionStats\n"
        "            | Command::GetCommands\n"
        "            | Command::GetAvailableModels => {\n"
        "                json!({ \"type\": self.kind() })\n"
        "            }\n"
        "            Command::SetThinkingLevel { level } => {\n"
        "                json!({ \"type\": self.kind(), \"level\": level })\n"
        "            }"
    )
    # model list parser next to ModelInfo
    s = s.replace(
        "/// Snapshot of `get_state` response data.",
        "/// Parse `get_available_models` data: {\"models\": [...]}\n"
        "pub fn parse_model_list(data: &Value) -> Vec<ModelInfo> {\n"
        "    data[\"models\"]\n"
        "        .as_array()\n"
        "        .map(|arr| arr.iter().filter_map(ModelInfo::parse).collect())\n"
        "        .unwrap_or_default()\n"
        "}\n"
        "\n"
        "/// Snapshot of `get_state` response data."
    )
    # tests
    s = s.replace(
        "    #[test]\n"
        "    fn agent_lifecycle() {",
        "    #[test]\n"
        "    fn model_list_parses_and_thinking_record_shape() {\n"
        "        let data = serde_json::json!({\"models\":[\n"
        "            {\"id\":\"glm-5.3-flash\",\"name\":\"GLM 5.3 Flash\",\"provider\":\"glm\",\"contextWindow\":200000},\n"
        "            {\"id\":\"m2\",\"name\":\"M2\",\"provider\":\"p\"}\n"
        "        ]});\n"
        "        let models = parse_model_list(&data);\n"
        "        assert_eq!(models.len(), 2);\n"
        "        assert_eq!(models[0].provider, \"glm\");\n"
        "        let c = Command::SetThinkingLevel { level: \"high\".into() };\n"
        "        assert_eq!(c.to_record(\"t1\"), json!({\"id\":\"t1\",\"type\":\"set_thinking_level\",\"level\":\"high\"}));\n"
        "    }\n"
        "\n"
        "    #[test]\n"
        "    fn agent_lifecycle() {"
    )
    io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
    print('protocol ok')
