pub type Result<T> = std::result::Result<T, failure::Error>;

pub trait RuntimeErrorNewImpl {
    fn new_impl(what: Self) -> RuntimeError;
}
#[derive(Debug)]
pub struct RuntimeError {
    what: String,
}
impl RuntimeError {
    pub fn new<T: RuntimeErrorNewImpl>(what: T) -> RuntimeError {
        T::new_impl(what)
    }
}
impl std::error::Error for RuntimeError {
    fn description(&self) -> &str {
        &self.what
    }
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.what)
    }
}
impl RuntimeErrorNewImpl for String {
    fn new_impl(what: String) -> RuntimeError {
        RuntimeError { what }
    }
}
impl RuntimeErrorNewImpl for &str {
    fn new_impl(what: &str) -> RuntimeError {
        RuntimeError {
            what: what.to_string(),
        }
    }
}

/// JSON値を文字列として取得
pub fn as_str(v: &serde_json::Value) -> Result<&str> {
    Ok(v.as_str()
        .ok_or_else(|| RuntimeError::new("Internal error: JSON value isn't string"))?)
}

/// JSON値を配列として取得
pub fn as_array(v: &serde_json::Value) -> Result<&Vec<serde_json::Value>> {
    Ok(v.as_array()
        .ok_or_else(|| RuntimeError::new("Internal error: JSON value isn't array"))?)
}

pub fn path_join(v: &[&str]) -> Result<String> {
    Ok(v.iter()
        .collect::<std::path::PathBuf>()
        .to_str()
        .ok_or_else(|| RuntimeError::new("Internal error: path -> str conversion failed"))?
        .to_string())
}
