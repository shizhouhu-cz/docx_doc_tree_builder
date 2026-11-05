//! 手写 JSON 序列化器（design.md §3.3）。
//!
//! - 零依赖，字段顺序稳定（利于 golden 测试 diff）
//! - None 字段直接省略 key（省字节）
//! - 数字手写 itoa（避免 fmt 机制进产物）
//! - sizePt：半磅值，偶数输出整数磅，奇数输出 "12.5" 形式

use crate::model::*;

/// 序列化入口：DocTree → JSON 字符串
pub fn to_json(t: &DocTree) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("{\"version\":\"1\"");
    if !t.styles.is_empty() {
        s.push_str(",\"styles\":[");
        for (i, si) in t.styles.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"id\":");
            esc(&si.id, &mut s);
            if let Some(name) = &si.name {
                s.push_str(",\"name\":");
                esc(name, &mut s);
            }
            s.push('}');
        }
        s.push(']');
    }
    s.push_str(",\"body\":[");
    write_blocks(&t.body, &mut s);
    s.push(']');
    if let Some(sec) = &t.section {
        s.push_str(",\"section\":{");
        let mut f = true;
        opt_i64("pageW", sec.page_w, &mut s, &mut f);
        opt_i64("pageH", sec.page_h, &mut s, &mut f);
        // 边距有任一存在才输出 margin 对象
        if sec.margin_top.is_some()
            || sec.margin_right.is_some()
            || sec.margin_bottom.is_some()
            || sec.margin_left.is_some()
        {
            comma(&mut s, &mut f);
            s.push_str("\"margin\":{");
            let mut g = true;
            opt_i64("top", sec.margin_top, &mut s, &mut g);
            opt_i64("right", sec.margin_right, &mut s, &mut g);
            opt_i64("bottom", sec.margin_bottom, &mut s, &mut g);
            opt_i64("left", sec.margin_left, &mut s, &mut g);
            s.push('}');
        }
        s.push('}');
    }
    s.push('}');
    s
}

fn comma(s: &mut String, first: &mut bool) {
    if !*first {
        s.push(',');
    }
    *first = false;
}

/// None 省略 key；Some 输出 `"name":value`
fn opt_i64(name: &str, v: Option<i64>, s: &mut String, first: &mut bool) {
    if let Some(v) = v {
        comma(s, first);
        s.push('"');
        s.push_str(name);
        s.push_str("\":");
        put_i64(v, s);
    }
}

fn opt_bool(name: &str, v: Option<bool>, s: &mut String, first: &mut bool) {
    if v == Some(true) {
        comma(s, first);
        s.push('"');
        s.push_str(name);
        s.push_str("\":true");
    }
}

fn opt_str(name: &str, v: Option<&str>, s: &mut String, first: &mut bool) {
    if let Some(v) = v {
        comma(s, first);
        s.push('"');
        s.push_str(name);
        s.push_str("\":");
        esc(v, s);
    }
}

fn opt_u32(name: &str, v: Option<u32>, s: &mut String, first: &mut bool) {
    if let Some(v) = v {
        comma(s, first);
        s.push('"');
        s.push_str(name);
        s.push_str("\":");
        put_i64(v as i64, s);
    }
}

fn write_blocks(bs: &[Block], s: &mut String) {
    for (i, b) in bs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        match b {
            Block::Paragraph(p) => write_para(p, s),
            Block::Table(t) => write_table(t, s),
        }
    }
}

fn write_para(p: &Paragraph, s: &mut String) {
    s.push_str("{\"type\":\"paragraph\"");
    if p.style.is_some() {
        s.push_str(",\"style\":");
        esc(p.style.as_deref().unwrap(), s);
    }
    if let Some(n) = &p.numbering {
        s.push_str(",\"numbering\":{\"id\":");
        put_i64(n.id as i64, s);
        s.push_str(",\"level\":");
        put_i64(n.level as i64, s);
        if let Some(m) = &n.marker {
            s.push_str(",\"marker\":");
            esc(m, s);
        }
        s.push('}');
    }
    if let Some(a) = &p.alignment {
        s.push_str(",\"alignment\":");
        esc(a, s);
    }
    let mut f2 = true;
    let any_ind = p.indent_left.is_some()
        || p.indent_right.is_some()
        || p.indent_first_line.is_some()
        || p.indent_hanging.is_some();
    if any_ind {
        s.push_str(",\"indent\":{");
        opt_i64("left", p.indent_left, s, &mut f2);
        opt_i64("right", p.indent_right, s, &mut f2);
        opt_i64("firstLine", p.indent_first_line, s, &mut f2);
        opt_i64("hanging", p.indent_hanging, s, &mut f2);
        s.push('}');
    }
    s.push_str(",\"children\":[");
    write_inlines(&p.children, s);
    s.push_str("]}");
}

fn write_table(t: &Table, s: &mut String) {
    s.push_str("{\"type\":\"table\",\"rows\":[");
    for (i, row) in t.rows.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"cells\":[");
        for (j, cell) in row.cells.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push('{');
            let mut f = true;
            if cell.col_span > 1 {
                comma(s, &mut f);
                s.push_str("\"colSpan\":");
                put_i64(cell.col_span as i64, s);
            }
            if cell.row_span > 1 {
                comma(s, &mut f);
                s.push_str("\"rowSpan\":");
                put_i64(cell.row_span as i64, s);
            }
            comma(s, &mut f);
            s.push_str("\"blocks\":[");
            write_blocks(&cell.blocks, s);
            s.push_str("]}");
        }
        s.push_str("]}");
    }
    s.push_str("]}");
}

fn write_inlines(cs: &[Inline], s: &mut String) {
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        match c {
            Inline::Text(t) => {
                s.push_str("{\"type\":\"text\",\"text\":");
                esc(&t.text, s);
                // 前面已手写首个键值对，后续字段需要逗号
                let mut f = false;
                opt_bool("bold", t.b, s, &mut f);
                opt_bool("italic", t.i, s, &mut f);
                opt_bool("underline", t.u, s, &mut f);
                opt_bool("strike", t.strike, s, &mut f);
                opt_str("color", t.color.as_deref(), s, &mut f);
                if let Some(sz) = t.sz {
                    comma(s, &mut f);
                    s.push_str("\"sizePt\":");
                    put_half_pt(sz, s);
                }
                opt_str("font", t.font.as_deref(), s, &mut f);
                s.push('}');
            }
            Inline::Tab => s.push_str("{\"type\":\"tab\"}"),
            Inline::Break(b) => {
                s.push_str("{\"type\":\"break\",\"breakType\":");
                let ty = match b {
                    BreakType::Line => "line",
                    BreakType::Page => "page",
                    BreakType::Column => "column",
                };
                esc(ty, s);
                s.push('}');
            }
            Inline::Hyperlink(h) => {
                s.push_str("{\"type\":\"hyperlink\",\"target\":");
                esc(&h.target, s);
                s.push_str(",\"children\":[");
                write_inlines(&h.children, s);
                s.push_str("]}");
            }
            Inline::Image(img) => {
                s.push_str("{\"type\":\"image\",\"target\":");
                esc(&img.target, s);
                let mut f = false;
                opt_str("name", img.name.as_deref(), s, &mut f);
                opt_u32("widthPx", img.width_px, s, &mut f);
                opt_u32("heightPx", img.height_px, s, &mut f);
                s.push('}');
            }
            Inline::FootnoteRef(id) => {
                s.push_str("{\"type\":\"footnoteRef\",\"id\":");
                put_i64(*id as i64, s);
                s.push('}');
            }
            Inline::Field(fl) => {
                s.push_str("{\"type\":\"field\",\"instr\":");
                esc(&fl.instr, s);
                s.push('}');
            }
        }
    }
}

/// JSON 字符串转义：`"`、`\`、控制字符；非 ASCII 直接输出 UTF-8
pub fn esc(v: &str, s: &mut String) {
    s.push('"');
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                s.push_str("\\u00");
                let b = c as u32;
                s.push(char::from_digit(b >> 4, 16).unwrap());
                s.push(char::from_digit(b & 0xF, 16).unwrap());
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

/// 手写 itoa：避免 fmt 机制进入 wasm 产物
pub fn put_i64(mut n: i64, s: &mut String) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let neg = n < 0;
    if !neg {
        n = -n; // 用负数域避免 i64::MIN 溢出
    }
    loop {
        i -= 1;
        buf[i] = b'0' + (-(n % 10)) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    s.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

/// 半磅 → sizePt：偶数输出整数，奇数输出 x.5
fn put_half_pt(h: u32, s: &mut String) {
    if h % 2 == 0 {
        put_i64((h / 2) as i64, s);
    } else {
        put_i64((h / 2) as i64, s);
        s.push_str(".5");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn itoa() {
        let mut s = String::new();
        put_i64(0, &mut s);
        put_i64(42, &mut s);
        put_i64(-7, &mut s);
        put_i64(i64::MIN, &mut s);
        put_i64(9223372036854775807, &mut s);
        assert_eq!(s, "042-7-92233720368547758089223372036854775807");
    }

    #[test]
    fn escaping() {
        let mut s = String::new();
        esc("a\"b\\c\nd\te\u{1}f中文", &mut s);
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\te\\u0001f中文\"");
    }

    #[test]
    fn half_pt() {
        let mut s = String::new();
        put_half_pt(24, &mut s);
        s.push(',');
        put_half_pt(25, &mut s);
        assert_eq!(s, "12,12.5");
    }
}
