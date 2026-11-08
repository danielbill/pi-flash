//! `appearance` section（001-1 §3）：主题 / 图标主题 / 三字体槽位 / 语言。
//! v1 平铺键同名直迁（字体是对象，保留原结构）。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

/// 字体槽位（family + pt size）。`None` 字段 = 未设置，默认值留在代码侧。
/// app 侧 `appearance::FontSpec` 即本类型 re-export（字段同形，调用点零改）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
}

/// 界面外观设置。clamp 唯一落点 = [`Section::validate`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Appearance {
    /// theme id（"mist" / "rose" / "one-light" / …，006 内置）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// icon theme id（内置集 = pi-web 自带 icons）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_theme: Option<String>,
    /// 会话字体（≈ zed UI font）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_font: Option<FontSpec>,
    /// 左 dock 功能面板字体
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_font: Option<FontSpec>,
    /// markdown/文件预览字体
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_font: Option<FontSpec>,
    /// 语言序号（i18n::TABLE 行；钳 0..=2，与 v1 读取 min(2) parity）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<u64>,
}

impl Section for Appearance {
    const KEY: &'static str = "appearance";

    fn validate(&mut self) {
        self.lang = self.lang.map(|v| v.min(2));
        for f in [
            &mut self.session_font,
            &mut self.panel_font,
            &mut self.markdown_font,
        ] {
            if let Some(spec) = f {
                // 字号统一钳制（2026-10-15 用户定夺 10–30，防手改存档离谱值）
                spec.size = spec.size.clamp(10.0, 30.0);
            }
        }
    }

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let pick_str = |k: &str| map.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let font = |k: &str| map.get(k).and_then(font_spec_from);
        let theme = pick_str("theme");
        let icon_theme = pick_str("icon_theme");
        let session_font = font("session_font");
        let panel_font = font("panel_font");
        let markdown_font = font("markdown_font");
        let lang = map.get("lang").and_then(|v| v.as_u64());
        if theme.is_none()
            && icon_theme.is_none()
            && session_font.is_none()
            && panel_font.is_none()
            && markdown_font.is_none()
            && lang.is_none()
        {
            return None;
        }
        Some(Self {
            theme,
            icon_theme,
            session_font,
            panel_font,
            markdown_font,
            lang,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let mut insert = |k: &str, v: Option<serde_json::Value>| {
            if let Some(v) = v {
                map.insert(k.to_string(), v);
            }
        };
        insert("theme", self.theme.clone().map(serde_json::Value::String));
        insert(
            "icon_theme",
            self.icon_theme.clone().map(serde_json::Value::String),
        );
        insert("session_font", self.session_font.as_ref().map(font_spec_to));
        insert("panel_font", self.panel_font.as_ref().map(font_spec_to));
        insert(
            "markdown_font",
            self.markdown_font.as_ref().map(font_spec_to),
        );
        insert("lang", self.lang.map(serde_json::Value::from));
    }
}

fn font_spec_from(v: &serde_json::Value) -> Option<FontSpec> {
    Some(FontSpec {
        family: v.get("family")?.as_str()?.to_string(),
        size: v.get("size")?.as_f64()? as f32,
    })
}

fn font_spec_to(spec: &FontSpec) -> serde_json::Value {
    serde_json::json!({ "family": spec.family, "size": spec.size })
}
