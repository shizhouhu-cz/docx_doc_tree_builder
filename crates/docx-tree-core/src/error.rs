//! 错误类型。刻意保持极简：核心库只需区分"一般错误"与"entry 不存在"。

/// 解析错误。`NotFound` 专用于 docx_entry 场景（对应 FFI 错误码 2）。
#[derive(Debug)]
pub enum Error {
    /// 一般解析错误，携带人可读描述
    Msg(String),
    /// 指定 entry 不存在
    NotFound,
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Error::Msg(m.into())
    }

    /// 取出错误消息（不经过 Display/fmt 机制，wasm 侧用于 set_err）
    pub fn take_msg(self) -> String {
        match self {
            Error::Msg(m) => m,
            Error::NotFound => String::from("entry not found"),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Msg(m) => f.write_str(m),
            Error::NotFound => f.write_str("entry not found"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;
