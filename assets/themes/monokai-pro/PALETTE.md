# Monokai Pro 色板来源（PALETTE.md）

PF 的 `monokai-pro` 主题仅取 **色值**（颜色值本身不受版权保护），移植自
Monokai Pro（monokai.pro，商业主题）VS Code 扩展的公开色板；**不复制、
不分发任何专有文件**（无扩展 JSON、无图标资产）。色值经社区移植三方
交叉核对，一致性一致：

| 用途 | 值 | 来源核对 |
|---|---|---|
| base0–base6 | `#19181a` `#221f22` `#2d2a2e` `#403e41` `#5b595c` `#727072` `#939293` | subtheme-dev/monokai-pro `colors/default.yaml`（注："pulled from the official vscode extension"）= helix `runtime/themes/monokai_pro.toml` palette = alacritty wiki |
| fg | `#fcfcfa` | 同上 + alacritty wiki Monokai Pro |
| 主色 red/orange/yellow/green/blue/purple | `#ff6188` `#fc9867` `#ffd866` `#a9dc76` `#78dce8` `#ab9df2` | 同上三源一致 |
| comment | `#727072` | 同上 |
| selection / current line | `#423f46` / `#3a383c` | tontinton/maki `monokai_pro.toml` |

PF 工作台槽位映射（hover `#363537`、nav `#323035`、accent_hover
`#ffe28c` 等派生值）为 PF 语义校准，见
`crates/app/src/appearance/theme.rs` 的 `MONOKAI_PRO` 文档注释；user_bg
取 selection `#423f46` 原值。

Monokai Pro 名称与设计归 Wimer Hazenberg / Monokai 所有；此移植为对
公开色板值的描述性引用。
