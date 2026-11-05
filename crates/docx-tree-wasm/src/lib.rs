//! docx-tree 的裸 FFI 绑定（design.md §3.4，编译目标 wasm32-unknown-unknown）。
//!
//! 错误码约定：
//! - 0 成功
//! - 1 一般解析错误（详情经 docx_err_msg 获取）
//! - 2 entry 不存在
//! - 3 panic（被 catch_unwind 拦截，不会跨 FFI 边界泄漏）
//! - 4 entry 名非 UTF-8
//!
//! 状态生命周期：docx_parse 成功后保留文件字节与中央目录（不释放），
//! docx_entry 复用它们按需解压单个 entry（图片字节即 v1 图片渲染数据源）；
//! 下次 docx_parse 覆盖旧状态。

use std::panic::{catch_unwind, AssertUnwindSafe};

use docx_tree_core::{self as core, zip::ZipArchive};

struct State {
    /// 上次 docx_parse 保留的文件字节
    retained: Vec<u8>,
    err: String,
}

// wasm 单线程环境，static mut + 原生指针访问（避免引用 static mut 的 lint）
static mut STATE: Option<State> = None;

fn state() -> &'static mut State {
    unsafe {
        let p = std::ptr::addr_of_mut!(STATE);
        if (*p).is_none() {
            *p = Some(State {
                retained: Vec::new(),
                err: String::new(),
            });
        }
        (*p).as_mut().unwrap_unchecked()
    }
}

fn set_err(msg: &str) {
    state().err = msg.to_string();
}

/// 分配 len 字节线性内存，返回指针（精确容量，配合 docx_free 重建 Vec）
#[no_mangle]
pub extern "C" fn docx_alloc(len: u32) -> *mut u8 {
    let len = len as usize;
    if len == 0 {
        return std::ptr::null_mut();
    }
    let v = vec![0u8; len];
    let ptr = v.as_ptr() as *mut u8;
    std::mem::forget(v);
    ptr
}

/// 释放 docx_alloc 分配的内存（按原长重建 Vec 后 drop）
#[no_mangle]
pub unsafe extern "C" fn docx_free(ptr: *mut u8, len: u32) {
    free_buf(ptr, len)
}

/// 释放结果缓冲（与 docx_free 同一实现，语义上区分便于 JS 侧审计）
#[no_mangle]
pub unsafe extern "C" fn docx_result_free(ptr: *mut u8, len: u32) {
    free_buf(ptr, len)
}

unsafe fn free_buf(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    drop(Vec::from_raw_parts(ptr, len as usize, len as usize));
}

/// 解析 docx：输入文件字节，输出 JSON UTF-8 字符串 (ptr,len)
#[no_mangle]
pub unsafe extern "C" fn docx_parse(
    input: *const u8,
    input_len: u32,
    out_ptr: *mut u32,
    out_len: *mut u32,
) -> u32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        parse_impl(input, input_len, out_ptr, out_len)
    }));
    match r {
        Ok(code) => code,
        Err(_) => {
            set_err("panic during parse");
            3
        }
    }
}

fn parse_impl(input: *const u8, input_len: u32, out_ptr: *mut u32, out_len: *mut u32) -> u32 {
    if input.is_null() || input_len == 0 {
        set_err("empty input");
        return 1;
    }
    if out_ptr.is_null() || out_len.is_null() {
        set_err("null out pointers");
        return 1;
    }
    let data = unsafe { std::slice::from_raw_parts(input, input_len as usize) };
    let st = state();
    // 保留字节 + 中央目录供 docx_entry 复用（下次 parse 覆盖）
    st.retained = data.to_vec();
    match core::parse(&st.retained) {
        Ok(tree) => {
            let json = core::to_json(&tree);
            write_out(out_ptr, out_len, json.as_bytes())
        }
        Err(e) => {
            set_err(&e.to_string());
            1
        }
    }
}

/// 按 entry 名提取解压字节（如 "word/media/image3.png"），依赖 docx_parse 保留的状态
#[no_mangle]
pub unsafe extern "C" fn docx_entry(
    name_ptr: *const u8,
    name_len: u32,
    out_ptr: *mut u32,
    out_len: *mut u32,
) -> u32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        entry_impl(name_ptr, name_len, out_ptr, out_len)
    }));
    match r {
        Ok(code) => code,
        Err(_) => {
            set_err("panic during entry");
            3
        }
    }
}

fn entry_impl(name_ptr: *const u8, name_len: u32, out_ptr: *mut u32, out_len: *mut u32) -> u32 {
    if name_ptr.is_null() || name_len == 0 {
        set_err("empty entry name");
        return 1;
    }
    if out_ptr.is_null() || out_len.is_null() {
        set_err("null out pointers");
        return 1;
    }
    let st = state();
    if st.retained.is_empty() {
        set_err("no document parsed yet");
        return 1;
    }
    let name = unsafe {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s,
            Err(_) => {
                set_err("entry name is not utf-8");
                return 4;
            }
        }
    };
    // 复用保留的文件字节重建目录（EOCD 扫描 ~64KB，成本可忽略）
    match ZipArchive::new(&st.retained) {
        Ok(zip) => match zip.read(name) {
            Ok(bytes) => write_out(out_ptr, out_len, &bytes),
            Err(docx_tree_core::Error::NotFound) => 2,
            Err(e) => {
                set_err(&e.take_msg());
                1
            }
        },
        Err(e) => {
            set_err(&e.to_string());
            1
        }
    }
}

/// 取回错误消息：拷入调用方缓冲，返回写入字节数（不含 NUL 终止符）
#[no_mangle]
pub unsafe extern "C" fn docx_err_msg(buf: *mut u8, cap: u32) -> u32 {
    if buf.is_null() || cap == 0 {
        return 0;
    }
    let st = state();
    let msg = st.err.as_bytes();
    let n = msg.len().min(cap as usize);
    std::ptr::copy_nonoverlapping(msg.as_ptr(), buf, n);
    n as u32
}

/// 把字节拷入新分配缓冲并写出 (ptr,len)；成功返回 0
fn write_out(out_ptr: *mut u32, out_len: *mut u32, bytes: &[u8]) -> u32 {
    let len = bytes.len() as u32;
    if len == 0 {
        unsafe {
            *out_ptr = 0;
            *out_len = 0;
        }
        return 0;
    }
    let ptr = docx_alloc(len);
    if ptr.is_null() {
        set_err("out of memory");
        return 1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        *out_ptr = ptr as u32;
        *out_len = len;
    }
    0
}
