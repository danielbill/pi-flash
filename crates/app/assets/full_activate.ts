import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

// full 档内置工具：bash,read,edit,write,grep,find,ls（无 powershell）。
const EXCLUDE = new Set(["powershell"]);
const FULL = ["bash", "read", "edit", "write", "grep", "find", "ls"];

/**
 * full+ 档（custom）的激活器（031 设计）。
 *
 * `-ne` 精确集把内置扩展也一起关掉，所以配方必须显式 `-e` 加回内置扩展；
 * 本扩展在 session_start 里把「full 内置工具 ∪ pi 自己算出的 active」设为
 * active —— 后者含 settings defaultTools 与「注册即激活」的 direct 插件
 * 工具，于是 deferred（MCP）/ model-only（codemode、tool_search）的默认
 * 非激活态与普通会话保持一致，不会被这一档强行拉直。
 *
 * **重挂恢复**（pi-flash-3x95，2026-11-03 三步探针实证）：custom 档 resume
 * 且不发 `--tools` 时，pi 跑 `_restoreToolsFromTranscript` ——
 * `active := 全分支 toolsAdded/toolsRemoved 回放后的当前集 ∪ 从未在
 * transcript 出现过的新注册工具`。中途卸载又重挂的扩展（pi-mcp-adapter、
 * pi-web-access 实测同症；pi-freeflow 无工具故「看起来正常」）：其工具在
 * 卸载轮被记入 toolsRemoved，重挂进程里既不在当前集、又因「出现过」不算
 * 新工具 → **注册但永不激活**，模型看不到（用户视角「挂不上」）。扩展侧
 * 无法自救：adapter 的 syncDirectTools 指纹未变跳过重注册（pi 对已知工具
 * 重注册也不激活），syncProxyTool 只补回它自己 deactivate 过的工具，对
 * pi 侧 restore 掐掉的无感。这里把「transcript 声明过（toolsAdded 出现过）
 * + 本进程已注册 + 注册即激活语义（exposure=direct）+ 尚未激活」的工具
 * 并回 —— 即 pi 本会给它们的默认态。收敛性：
 * - deferred / hidden / model-only 不动（tool_search 与扩展自管）；
 * - adapter lazy 工具由其 `before_agent_start` holdLazyToolsInactive
 *   每轮兜底压回，不会被这条规则顶开；
 * - 全新会话无 system 消息（declared 空）→ 天然 no-op，首挂行为不变；
 * - 动态激活类扩展（web-access dynamic）未激活过的工具从未入 transcript
 *   → 不会被误拉直；
 * - 扩展面板主动禁用的服务器走 unregisterTool（registry 已无）→ 不会
 *   被并回。
 * 本扩展按 -e 顺序恒排在最后（tools_recipe::args_from），跑在所有包扩展
 * 的 session_start 之后，union 的是它们处理完的现场。
 */
export default function (pi: ExtensionAPI) {
  pi.on("session_start", (_event: unknown, ctx: ExtensionContext) => {
    const names = new Set<string>(
      [...FULL, ...pi.getActiveTools()].filter((n) => !EXCLUDE.has(n)),
    );
    try {
      // transcript 曾声明过的工具名（toolsAdded 并集；被 remove 过的必然
      // 先 add 过，只收 add 侧即可）
      const declared = new Set<string>();
      for (const entry of ctx.sessionManager.getBranch()) {
        if (entry.type !== "message" || entry.message.role !== "system") continue;
        const added = (entry.message as { toolsAdded?: Array<{ name: string }> })
          .toolsAdded;
        for (const tool of added ?? []) {
          if (tool.name) declared.add(tool.name);
        }
      }
      if (declared.size > 0) {
        for (const tool of pi.getAllTools() ?? []) {
          if (
            !EXCLUDE.has(tool.name) &&
            tool.exposure === "direct" &&
            declared.has(tool.name) &&
            !names.has(tool.name)
          ) {
            names.add(tool.name);
          }
        }
      }
    } catch {
      // transcript 读不到时保持旧行为：FULL ∪ active（诊断探针 probe2 的
      // 无差别 direct 并回也能修，但会误拉直 dynamic 模式未激活的工具）
    }
    pi.setActiveTools([...names]);
  });
}
