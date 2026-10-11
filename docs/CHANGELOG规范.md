# CHANGELOG规范

## 双语机制（中英对齐到同一个 GitHub Release）

`docs/CHANGELOG.md`（中文，**权威版**）与 `docs/CHANGELOG_EN.md`（英文，派生译文）两个文件，通过**同一份发布说明文件**对齐到同一个 GitHub Release：

```
tmp/release-notes-<v>.md（单一事实来源，双语）
├─ <!-- EN --> 之前的中文段 ──→ release.sh 收编 docs/CHANGELOG.md   [v] 段
├─ <!-- EN --> 之后的英文段 ──→ release.sh 收编 docs/CHANGELOG_EN.md [v] 段
└─ 全文（中 + 英）──────────→ gh release create --notes-file → Release v<v> 正文双语
```

- 同一个 tag 下：Release 正文 = 两份 CHANGELOG 的同版本段（都出自同一份 notes，由 release.sh 单点收编）。
- `<!-- EN -->` 单独占一行，是 HTML 注释，GitHub 渲染 Release 正文时不可见。
- 发版时序：**发版前人工校验中文段，确认后 agent 翻译追加英文段**，再跑 release.sh；纯中文也可发版（降级路径），发版后用 `gh release edit v<v> --notes-file <双语文件>` 补挂英文。

## 内容格式（中英文段各自遵循）

- 安装说明
- 新增（new）
- 修复（bug fix）
- 改进（improve）

## 发布说明文件示例（双语）

`````markdown
## [0.1.0] - 2026-10-10

pi-flash 首个正式对外版本。

### 安装与更新

```bash
npm install -g pi-flash@latest   
```
前置需求： Node.js ≥ 22.19。

### 新增
- 文件浏览器默认显示在会话列表下方
- 新命令行命令：pi-web version、update、status、stop、open。(#1139, #1154)

### 修复
- 侧边栏：点击「显示更多」后选中最后一个会话，不再露出更旧的会话；折叠分组会丢弃它的「显示更多」窗口。(51dda7c, 0066d5f)
- 运行中写入的文件、以及在编辑器或终端里的改动，文件树现在会自动刷新（运行时与窗口获得焦点时）。(#1144, #1153)

### 改进
- 侧边栏样式：会话行 28px，文字只用三种颜色，选中行有 3px 强调条，文件区头部按键顺序与文件标签一致、按键更大。(4fa6799, 02532c3, cae6201)

<!-- EN -->
## [0.1.0] - 2026-10-10

First official public release of pi-flash.

### Install & Update

```bash
npm install -g pi-flash@latest
```
Requires: Node.js ≥ 22.19.

### New
- File browser shown below the session list by default
- New CLI commands: pi-web version, update, status, stop, open. (#1139, #1154)

### Bug fixes
- Sidebar: after clicking "show more", the last session is selected without revealing older ones; collapsing a group no longer drops its "show more" window. (51dda7c, 0066d5f)
- Files written while a task runs — and edits made in the editor or terminal — now refresh the file tree automatically (on runtime events and window focus). (#1144, #1153)

### Improvements
- Sidebar styling: 28px session rows, three text colors only, a 3px accent bar on the selected row; file-area header key order matches file tabs with larger keys. (4fa6799, 02532c3, cae6201)
`````

（英文段首行重复版本标题，便于两份 CHANGELOG 收编后各自结构完整。）
