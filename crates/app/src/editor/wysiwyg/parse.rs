//! pulldown-cmark → `Vec<LineModel>`：行级样式（heading/quote/list）+
//! 行内 span（strong/em/code/link，doc offset 坐标）+ 该行 folds（隐藏语法
//! range，已按 reveal 过滤）。
//!
//! 容错策略：语法未闭合 / 范围存疑（`**a**b` 类分隔符歧义）→ 不折叠不加
//! 样式，保守显示源码。fixtures 见模块单测（嵌套/未闭合/转义/中英混排）。
