import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// full 档内置工具：bash,read,edit,write,grep,find,ls（无 powershell）。
const EXCLUDE = new Set(["powershell"]);
const FULL = ["bash", "read", "edit", "write", "grep", "find", "ls"];

/**
 * full+plugins 档的激活器（031 设计）。
 *
 * `-ne` 精确集把内置扩展也一起关掉，所以配方必须显式 `-e` 加回内置扩展；
 * 本扩展在 session_start 里把「full 内置工具 ∪ pi 自己算出的 active」设为
 * active —— 后者含 settings defaultTools 与「注册即激活」的 direct 插件
 * 工具，于是 deferred（MCP）/ model-only（codemode、tool_search）的默认
 * 非激活态与普通会话保持一致，不会被这一档强行拉直。
 */
export default function (pi: ExtensionAPI) {
  pi.on("session_start", () => {
    const names = new Set<string>([...FULL, ...pi.getActiveTools()]);
    pi.setActiveTools([...names].filter((n) => !EXCLUDE.has(n)));
  });
}
