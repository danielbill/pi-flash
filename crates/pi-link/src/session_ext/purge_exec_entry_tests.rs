use super::*;

#[test]
fn matches_market_versioned_dev_flat_and_rejects_others() {
    let id = "pf-wxbot";
    // market：@scope__id + 版本层
    assert!(is_plugin_pi_entry(
        "c:/r/plugins/@pi-flash-plugins__pf-wxbot/0.1.3/pi/index.mjs",
        id
    ));
    // dev 平铺：<id>/pi/
    assert!(is_plugin_pi_entry("c:/r/plugins/pf-wxbot/pi/index.mjs", id));
    // 别的插件（__ 前缀不同于 id）：不匹配
    assert!(!is_plugin_pi_entry(
        "c:/r/plugins/@x__wxbotpro/0.1.0/pi/index.mjs",
        id
    ));
    // 本插件但非 pi 入口（data/svc 通道文件）：不匹配
    assert!(!is_plugin_pi_entry(
        "c:/r/plugins/@pi-flash-plugins__pf-wxbot/0.1.3/data/svc/in",
        id
    ));
    // 无 /plugins/ 段：不匹配
    assert!(!is_plugin_pi_entry("npm:pi-freeflow", id));
}
