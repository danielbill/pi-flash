//! v57 富渲染组件（各自独立，markdown.rs 只留薄胶水）：
//! - html.rs：raw HTML 安全子集 → markdown 模型
//! - math.rs：KaTeX 兼容数学渲染（RaTeX 管线）[v57-2]
//! - mermaid.rs：mermaid 图渲染 [v57-3]

pub(crate) mod html;
