//! Golden 集成测试：合成 docx → JSON，与 fixtures/golden/sample.json 逐字节对比。
//! golden diff 是回归主防线（design.md §7）。
//!
//! 更新 golden：设环境变量 DOCX_UPDATE_GOLDEN=1 后重跑。

mod common;

use common::sample_docx;

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/golden/sample.json"
);

#[test]
fn golden_sample() {
    let docx = sample_docx();
    let tree = docx_tree_core::parse(&docx).expect("parse");
    let json = docx_tree_core::to_json(&tree);
    if std::env::var("DOCX_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN_PATH).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN_PATH, &json).unwrap();
    }
    let expected = std::fs::read_to_string(GOLDEN_PATH).unwrap();
    assert_eq!(
        json, expected,
        "golden diff（如为有意变更请 DOCX_UPDATE_GOLDEN=1 重生成）"
    );
}

/// 导出合成样本 docx 到 fixtures/（demo 页面与手工验证用）：
/// `cargo test -p docx-tree-core --test golden -- --ignored`
#[test]
#[ignore = "仅手动导出样本时运行"]
fn export_sample_docx() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/sample.docx");
    std::fs::write(path, sample_docx()).expect("write fixture");
}

/// 结构级断言：不依赖 golden 文本，直接验证关键语义
#[test]
fn structural_semantics() {
    let docx = sample_docx();
    let tree = docx_tree_core::parse(&docx).expect("parse");
    let json = docx_tree_core::to_json(&tree);

    // 有效格式：标题样式链 docDefaults(10.5pt) → Normal(12pt) → Heading1(16pt/bold/color)
    assert!(json.contains(r#""style":"Heading1""#));
    assert!(json.contains(r#""alignment":"center""#));
    assert!(json.contains(r#""bold":true,"color":"1F4E79","sizePt":16"#));
    // docDefaults 半磅：10.5pt
    assert!(json.contains(r#""sizePt":10.5"#));
    // 编号 marker：1. / a) / 2.（level1 计数器在回到 level0 后重置）
    assert!(json.contains(r#""numbering":{"id":1,"level":0,"marker":"1."}"#));
    assert!(json.contains(r#""numbering":{"id":1,"level":1,"marker":"a)"}"#));
    assert!(json.contains(r#""numbering":{"id":1,"level":0,"marker":"2."}"#));
    // bullet：空 marker
    assert!(json.contains(r#""numbering":{"id":2,"level":0,"marker":""}"#));
    // 文本合并 + 实体反转义（JSON 只转义必要字符，&/> 原样输出）
    assert!(json.contains(r#""text":"加粗带空格 ""#));
    assert!(json.contains(r#""text":"A&B>C""#));
    // 超链接 target 已解析
    assert!(json.contains(r#""target":"https://example.com/""#));
    // 图片：entry 路径 + EMU→px + docPr name
    assert!(
        json.contains(r#""type":"image","target":"word/media/image1.png","name":"图片1","widthPx":100,"heightPx":10"#)
    );
    // AlternateContent：取 Choice 分支（名"Choice分支图"），Fallback 被跳过
    assert!(json.contains(r#""name":"Choice分支图""#));
    // sdt 透明下钻
    assert!(json.contains(r#""text":"sdt内部段落""#));
    // 表格合并：colSpan 2；vMerge restart → rowSpan 2；continue 单元格被移除
    assert!(json.contains(r#""colSpan":2"#));
    assert!(json.contains(r#""rowSpan":2"#));
    assert!(json.contains(r#""text":"33""#));
    // 域指令 + 脚注引用
    assert!(json.contains(r#""type":"field","instr":"PAGE ""#));
    assert!(json.contains(r#""type":"footnoteRef","id":2"#));
    // sectPr
    assert!(json.contains(r#""section":{"pageW":11906,"pageH":16838,"margin":{"top":1440,"right":1800,"bottom":1440,"left":1800}}"#));
    // 样式索引（供渲染层 heading 映射）
    assert!(json.contains(
        r#""styles":[{"id":"Normal","name":"Normal"},{"id":"Heading1","name":"heading 1"}]"#
    ));

    let _ = tree;
}

/// docx_entry 场景：parse 后按 entry 名提取图片字节（v1 图片渲染数据源）
#[test]
fn entry_extraction() {
    let docx = sample_docx();
    let tree = docx_tree_core::parse(&docx).unwrap();
    // 找出树中的图片 target
    let img_target = find_image_target(&tree.body).expect("image in tree");
    let zip = docx_tree_core::zip::ZipArchive::new(&docx).unwrap();
    let bytes = zip.read(&img_target).expect("entry bytes");
    assert_eq!(bytes.as_slice(), common::PNG_1PX);
}

fn find_image_target(blocks: &[docx_tree_core::model::Block]) -> Option<String> {
    use docx_tree_core::model::{Block, Inline};
    fn in_inlines(cs: &[Inline]) -> Option<String> {
        for c in cs {
            if let Inline::Image(i) = c {
                return Some(i.target.clone());
            }
            if let Inline::Hyperlink(h) = c {
                if let Some(t) = in_inlines(&h.children) {
                    return Some(t);
                }
            }
        }
        None
    }
    for b in blocks {
        if let Block::Paragraph(p) = b {
            if let Some(t) = in_inlines(&p.children) {
                return Some(t);
            }
        }
        if let Block::Table(t) = b {
            for r in &t.rows {
                for c in &r.cells {
                    if let Some(t) = find_image_target(&c.blocks) {
                        return Some(t);
                    }
                }
            }
        }
    }
    None
}
