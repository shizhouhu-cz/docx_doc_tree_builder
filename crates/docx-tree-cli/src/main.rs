//! 原生调试 CLI（仅开发/测试用，不进 wasm 产物）。
//!
//! 用法：
//!   docx-tree <file.docx>                # 解析并输出 JSON 到 stdout
//!   docx-tree --entry <name> <file.docx>  # 提取 zip entry 原始字节到 stdout

use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if args.len() == 3 && args[0] == "--entry" {
        let name = &args[1];
        let path = &args[2];
        let data = read_file(path)?;
        let zip = docx_tree_core::zip::ZipArchive::new(&data).map_err(|e| e.to_string())?;
        let bytes = zip.read(name).map_err(|e| format!("{name}: {e}"))?;
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string())?;
        return Ok(ExitCode::SUCCESS);
    }
    if args.len() == 1 {
        let data = read_file(&args[0])?;
        let tree = docx_tree_core::parse(&data).map_err(|e| e.to_string())?;
        let json = docx_tree_core::to_json(&tree);
        println!("{json}");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("usage: docx-tree <file.docx> | docx-tree --entry <name> <file.docx>");
    Ok(ExitCode::from(2))
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("{path}: {e}"))?;
    Ok(buf)
}
