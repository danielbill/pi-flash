# 更新日志

所有对外发布的版本变化记录于此，格式见 `docs/CHANGELOG规范.md`。

## [0.1.0] - 2026-10-10

pi-flash 首个正式对外版本：Rust + GPUI 实现的极速 pi coding agent 桌面端。

### 安装与更新

```bash
npm install -g pi-flash@latest   # 首次安装与更新同命令
pi-flash
```

需要 Node.js ≥ 22.19。
也可直接下载 zip 绿色包，解压后双击 `pi-flash.exe` 即用，无需 Node。

### 新增
- 会话核心：流式渲染、多轮对话、中断、分支与完整历史
- 输入与编辑：Markdown 源码/预览双态、插图自动落盘、内置终端、文件树与 git 面板
- 扩展管理：模型 / 技能 / 插件 / MCP（钉版 pi 随应用分发，不读系统 PATH）
- 设置：界面 / 主题 / 多语言 / 远程控制（微信）
- 分发：npm 一条命令安装与更新；macOS arm64 包由 CI 构建并挂载至 Release

### 修复
- 文件树 / 设置·可用模型列表滚不动：滚动句柄每帧重建导致偏移归零，改为按 id 复用

### 改进
- 提示音换成 pi-web 原版音色（Rust 侧合成 WAV 播放，替代系统提示音）
