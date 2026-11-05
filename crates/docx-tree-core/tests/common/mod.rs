//! 集成测试公共：合成 docx 样本（手工拼 zip，stored + deflate 混合）。
//! 不依赖外部工具，保证测试可离线复现。

/// 构造 zip：parts 为 (entry 名, 数据, 是否 deflate)
pub fn build_zip(parts: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut cdir: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();

    for (name, data, deflate) in parts {
        offsets.push(out.len() as u32);
        let (method, payload) = if *deflate {
            (8u16, miniz_oxide::deflate::compress_to_vec(data, 6))
        } else {
            (0u16, data.to_vec())
        };
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // time/date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc（本库不校验）
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&payload);
    }

    let cd_off = out.len() as u32;
    for (i, (name, data, deflate)) in parts.iter().enumerate() {
        let (method, payload) = if *deflate {
            (8u16, miniz_oxide::deflate::compress_to_vec(data, 6))
        } else {
            (0u16, data.to_vec())
        };
        cdir.extend_from_slice(b"PK\x01\x02");
        cdir.extend_from_slice(&20u16.to_le_bytes());
        cdir.extend_from_slice(&20u16.to_le_bytes());
        cdir.extend_from_slice(&0u16.to_le_bytes()); // flags
        cdir.extend_from_slice(&method.to_le_bytes());
        cdir.extend_from_slice(&0u32.to_le_bytes()); // time/date
        cdir.extend_from_slice(&0u32.to_le_bytes()); // crc
        cdir.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        cdir.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cdir.extend_from_slice(&(name.len() as u16).to_le_bytes());
        cdir.extend_from_slice(&0u16.to_le_bytes()); // extra
        cdir.extend_from_slice(&0u16.to_le_bytes()); // comment
        cdir.extend_from_slice(&0u16.to_le_bytes()); // disk start
        cdir.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        cdir.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        cdir.extend_from_slice(&offsets[i].to_le_bytes());
        cdir.extend_from_slice(name.as_bytes());
    }

    out.extend_from_slice(&cdir);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
    out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cdir.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

pub const PNG_1PX: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

pub fn styles_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="21"/></w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults>
<w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/><w:rPr><w:sz w:val="24"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:color w:val="1F4E79"/></w:rPr></w:style>
</w:styles>"#
    .to_string()
}

pub fn numbering_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val=""/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#
    .to_string()
}

pub fn rels_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/>
<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#
    .to_string()
}

pub fn document_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>标题：测试</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>第一项</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>子项</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>第二项</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>项目符号</w:t></w:r></w:p>
<w:p><w:r><w:t>普通</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">加粗带空格 </w:t></w:r><w:r><w:rPr><w:i/><w:sz w:val="30"/></w:rPr><w:t>斜体15pt</w:t></w:r><w:r><w:tab/></w:r><w:r><w:br/></w:r><w:r><w:t>A&amp;B&gt;C</w:t></w:r></w:p>
<w:p><w:hyperlink r:id="rId7"><w:r><w:t>链接文字</w:t></w:r></w:hyperlink></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:extent cx="952500" cy="95250"/><wp:docPr id="1" name="图片1"/><a:graphic><a:graphicData><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="img.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId8"/></pic:blipFill><pic:spPr/></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:inline><wp:extent cx="190500" cy="190500"/><wp:docPr id="2" name="Choice分支图"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId8"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape xmlns:v="urn:x-vml" style="width:10pt"><v:imagedata r:id="rId8"/></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>
<w:sdt><w:sdtPr><w:id w:val="1"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>sdt内部段落</w:t></w:r></w:p></w:sdtContent></w:sdt>
<w:tbl>
<w:tr><w:tc><w:p><w:r><w:t>11</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>12与13</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>竖排</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>22</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>32</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>33</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:jc w:val="center"/><w:ind w:left="720" w:firstLine="360"/></w:pPr></w:p>
<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p>
<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800"/></w:sectPr>
</w:body>
</w:document>"#
    .to_string()
}

/// 合成完整样本 docx（document.xml 用 deflate，其余 stored，模拟真实混合）
pub fn sample_docx() -> Vec<u8> {
    build_zip(&[
        ("word/document.xml", document_xml().as_bytes(), true),
        ("word/_rels/document.xml.rels", rels_xml().as_bytes(), false),
        ("word/styles.xml", styles_xml().as_bytes(), false),
        ("word/numbering.xml", numbering_xml().as_bytes(), false),
        ("word/media/image1.png", PNG_1PX, false),
    ])
}
