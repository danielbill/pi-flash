//! content_blocks 图片坐标标注剥除（pi formatDimensionNote）形状锁。

use crate::protocol::*;
use serde_json::json;

#[test]
fn image_dimension_note_stripped_from_text_blocks() {
    // pi formatDimensionNote 实录（会话 jsonl 摘例）：user 附件尾拼 \n\n + 标注
    let content = json!([
        {"type":"text","text":"这是截图，为什么有2个replysession？\n\n[Image: original 2295x561, displayed at 2000x489. Multiply coordinates by 1.15 to map to original image.]"},
        {"type":"image","data":"QUJD","mimeType":"image/png"}
    ]);
    let blocks = content_blocks(&content);
    assert_eq!(
        blocks[0],
        Block::Text {
            content_index: 0,
            text: "这是截图，为什么有2个replysession？".into(),
        }
    );

    // read 工具图结果：前缀行 + 标注，同样剥
    let content = json!([{"type":"text","text":"Read image file [image/png]\n[Image: original 2560x1368, displayed at 2000x1069. Multiply coordinates by 1.28 to map to original image.]"}]);
    let blocks = content_blocks(&content);
    assert_eq!(
        blocks[0],
        Block::Text {
            content_index: 0,
            text: "Read image file [image/png]".into(),
        }
    );

    // 无标注文本原样保留
    let content = json!("普通文本");
    assert_eq!(
        content_blocks(&content)[0],
        Block::Text {
            content_index: 0,
            text: "普通文本".into(),
        }
    );

    // 正文中间出现的同形文本不动（只剥尾部）
    let content = json!([{"type":"text","text":"看 [Image: original 10x10, displayed at 5x5. Multiply coordinates by 2.00 to map to original image.] 这个"}]);
    assert_eq!(
        content_blocks(&content)[0],
        Block::Text {
            content_index: 0,
            text: "看 [Image: original 10x10, displayed at 5x5. Multiply coordinates by 2.00 to map to original image.] 这个".into(),
        }
    );

    // 形状不符（手打同前缀、缺数字/缺尾句）不剥
    let content = json!([{"type":"text","text":"[Image: original 尺寸没写对]"}]);
    assert_eq!(
        content_blocks(&content)[0],
        Block::Text {
            content_index: 0,
            text: "[Image: original 尺寸没写对]".into(),
        }
    );

    // 多图多条连排全剥
    let content = json!([{"type":"text","text":"两张图\n\n[Image: original 2370x162, displayed at 2000x137. Multiply coordinates by 1.19 to map to original image.]\n\n[Image: original 1000x800, displayed at 1000x800. Multiply coordinates by 1.00 to map to original image.]"}]);
    assert_eq!(
        content_blocks(&content)[0],
        Block::Text {
            content_index: 0,
            text: "两张图".into(),
        }
    );
}
