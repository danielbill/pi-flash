// 扩展/技能说明 token 探针（040 延迟加载）v2。
//
// 复用 pi 自己的机器测「装一个包要吃多少上下文」，三块全算：
//   1. 提示词差值：pi 的 `loadExtensions`（jiti/别名/registerTool 全套）装包，
//      `buildSystemPromptSections` + `estimateTokens` 算「相对内置 7 件套」
//      的工具 promptSnippet/promptGuidelines 边际增量；
//   2. 调用声明：每颗**活跃**工具（defaultActive:false 除外）的
//      name+description+JSON schema —— 模型每次请求都收到的 tools 数组；
//   3. 指令文本：会话期才注册的大段说明。手动调用扩展挂在
//      `before_agent_start` 上的 handler（读它写进 event.systemPromptOptions
//      .sections 的内容），并向自建 bus emit `pi:instruction-groups`
//      （新版 pi 的收集器通道，extension 自身会用 isManaged 去重，两条
//      通道同时存在也不会重复计）。
//
// 用法：node token_probe.mjs <input.json>
//   input: { pi_dist, cwd, packages: [{ source, dir }] }
// 输出（stdout，纯 JSON）：
//   { results: [{ source, ext_tokens, skills_tokens,
//                 prompt_tokens, decl_tokens, instruct_tokens, error? }] }
//   ext_tokens = prompt_tokens + decl_tokens + instruct_tokens（展示用合计）
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, resolve as pathResolve, basename } from "node:path";
import { pathToFileURL } from "node:url";

const input = JSON.parse(readFileSync(process.argv[2], "utf8"));
const imp = (p) => import(pathToFileURL(p).href);

const sysprompt = await imp(join(input.pi_dist, "core/system-prompt.js"));
const { estimateTokens } = await imp(join(input.pi_dist, "core/compaction/compaction.js"));
const { readPiManifest } = await imp(join(input.pi_dist, "core/pi-manifest.js"));
const toolsMod = await imp(join(input.pi_dist, "core/tools/index.js"));
const { loadExtensions, createExtensionRuntime } = await imp(
  join(input.pi_dist, "core/extensions/loader.js"),
);
const { createEventBus } = await imp(join(input.pi_dist, "core/event-bus.js"));

// 基线 = full 档的内置 7 件（read/bash/edit/write/grep/find/ls）
const baseDefs = [
  toolsMod.createReadToolDefinition(input.cwd),
  toolsMod.createBashToolDefinition(input.cwd),
  toolsMod.createEditToolDefinition(input.cwd),
  toolsMod.createWriteToolDefinition(input.cwd),
  toolsMod.createGrepToolDefinition(input.cwd),
  toolsMod.createFindToolDefinition(input.cwd),
  toolsMod.createLsToolDefinition(input.cwd),
];

// 会话期 handler 可能摸 ctx 的各种方法（isProjectTrusted 等）——已知字段给
// 真值，其余一律兜底成 no-op 函数，让指令文本组装走默认分支。
const fakeCtx = () =>
  new Proxy(
    { cwd: input.cwd, globalCwd: input.cwd, isProjectTrusted: () => false },
    {
      get(target, prop) {
        if (prop in target) return target[prop];
        return () => undefined;
      },
    },
  );
const tokens = (text) => estimateTokens({ role: "system", content: text });
const flush = () => new Promise((r) => setImmediate(r));

/** 定义集 → 系统提示词 token 数（与真实会话同一组装器 + 估算器）。 */
function promptTokens(defs, skills) {
  const snippets = {};
  const guidelines = {};
  for (const d of defs) {
    if (typeof d.promptSnippet === "string" && d.promptSnippet) {
      snippets[d.name] = d.promptSnippet;
    }
    if (Array.isArray(d.promptGuidelines) && d.promptGuidelines.length > 0) {
      guidelines[d.name] = [...d.promptGuidelines];
    }
  }
  const sections = sysprompt.buildSystemPromptSections({
    selectedTools: defs.map((d) => d.name),
    toolSnippets: snippets,
    toolGuidelines: guidelines,
    skills,
    cwd: input.cwd,
  });
  return tokens(Object.values(sections).join("\n\n"));
}

/** 调用声明：活跃工具的 name+description+schema（模型每次请求都收到）。 */
function declTokens(defs) {
  let n = 0;
  for (const d of defs) {
    if (d.defaultActive === false) continue;
    n += tokens(`${d.name}\n${d.description ?? ""}\n${JSON.stringify(d.parameters ?? {})}`);
  }
  return n;
}

/** 包的扩展入口（pi 同款解析：pi.extensions 清单 → index.ts/js → 目录扫描）。 */
function resolveEntries(dir) {
  const pj = join(dir, "package.json");
  if (existsSync(pj)) {
    const manifest = readPiManifest(pj);
    if (manifest?.extensions?.length) {
      const entries = manifest.extensions
        .map((e) => pathResolve(dir, e))
        .filter((p) => existsSync(p));
      if (entries.length > 0) return entries;
    }
  }
  for (const name of ["index.ts", "index.js"]) {
    const p = join(dir, name);
    if (existsSync(p)) return [p];
  }
  const out = [];
  const walk = (d, depth) => {
    if (depth > 3 || out.length > 50) return;
    let list;
    try {
      list = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of list) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      const full = join(d, e.name);
      if (e.isDirectory()) walk(full, depth + 1);
      else if (/\.(ts|js)$/.test(e.name)) out.push(full);
    }
  };
  walk(dir, 0);
  return out;
}

/** 包内 skills（pi.extensions 清单 → SKILL.md frontmatter 的 name/description）。 */
function packageSkills(dir) {
  const pj = join(dir, "package.json");
  let listed = null;
  try {
    listed = readPiManifest(pj)?.skills ?? null;
  } catch {
    listed = null;
  }
  const skills = [];
  const take = (mdPath) => {
    if (!existsSync(mdPath)) return;
    try {
      const raw = readFileSync(mdPath, "utf8");
      const m = /^---\r?\n([\s\S]*?)\r?\n---/.exec(raw);
      const fm = m?.[1] ?? "";
      const pick = (key) => {
        const km = new RegExp(`^${key}:\\s*(.+)$`, "m").exec(fm);
        return km?.[1]?.trim().replace(/^["']|["']$/g, "");
      };
      const name = pick("name") ?? basename(dirname(mdPath));
      const description = pick("description") ?? "";
      if (description) skills.push({ name, description, filePath: mdPath });
    } catch {
      // 读不了的 skill 不计入
    }
  };
  for (const entry of listed ?? []) {
    const p = pathResolve(dir, entry);
    if (!existsSync(p)) continue;
    if (p.endsWith(".md")) take(p);
    else take(join(p, "SKILL.md"));
  }
  return skills;
}

const baseTokens = promptTokens(baseDefs, []);
const results = [];
for (const pkg of input.packages) {
  try {
    const entries = resolveEntries(pkg.dir);
    const skills = packageSkills(pkg.dir);

    // ---- 装载 + 会话期通道捕获（同一包一个 runtime/bus，互不串线） ----
    const bus = createEventBus();
    const groups = [];
    const collector = {
      register(group) {
        if (group) groups.push(group);
      },
      isManaged() {
        return groups.length > 0;
      },
    };
    const runtime = createExtensionRuntime();
    const loaded = await loadExtensions(entries, input.cwd, bus, runtime);
    const { extensions, errors } = loaded;
    if (extensions.length === 0) {
      // 整包装载失败（常见：缺依赖）——报错而不是误报 0
      throw new Error(errors[0]?.error ?? "no extensions loaded");
    }

    // 指令组（新版 pi 通道）：handler 已挂在我们的 bus 上，emit 一个 collector
    bus.emit("pi:instruction-groups", collector);
    await flush();
    let instructText = "";
    for (const g of groups) {
      try {
        const t =
          typeof g.instructions === "function"
            ? await g.instructions(fakeCtx())
            : g.instructions;
        if (typeof t === "string") instructText += `${t}\n`;
      } catch {
        // 单组取文失败不影响其它
      }
    }

    // before_agent_start 兜底通道：手动调 handler，读它写进 sections 的文本
    // （extension 自身的 isManaged 判断保证与指令组通道不重复计）
    const sections = {};
    for (const ext of extensions) {
      for (const h of ext.handlers.get("before_agent_start") ?? []) {
        try {
          const event = { systemPromptOptions: { sections } };
          await h(event, fakeCtx());
        } catch {
          // handler 内部异常（如读配置失败）不阻断
        }
      }
    }
    instructText += Object.values(sections)
      .filter((v) => typeof v === "string")
      .join("\n");

    // ---- 计数 ----
    const defs = extensions.flatMap((e) => [...e.tools.values()].map((t) => t.definition));
    const promptTokensN = Math.max(promptTokens(baseDefs.concat(defs), []) - baseTokens, 0);
    const declTokensN = declTokens(defs);
    const instructTokensN = instructText ? Math.max(tokens(instructText), 0) : 0;
    const skillsTokensN =
      skills.length > 0 ? Math.max(promptTokens(baseDefs, skills) - baseTokens, 0) : 0;
    results.push({
      source: pkg.source,
      prompt_tokens: promptTokensN,
      decl_tokens: declTokensN,
      instruct_tokens: instructTokensN,
      ext_tokens: promptTokensN + declTokensN + instructTokensN,
      skills_tokens: skillsTokensN,
    });
  } catch (e) {
    results.push({ source: pkg.source, error: String(e?.message ?? e) });
  }
}
process.stdout.write(JSON.stringify({ results }));
