use byte_str::ByteStr;
use core::fmt;

const DELIMITER: &str = "\nmc_";

pub struct ToolId {
    pub tool_call_id: ByteStr,
    pub model_call_id: Option<ByteStr>,
}

impl ToolId {
    pub fn parse(s: ByteStr) -> Self {
        if let Some((tool_call_id, model_call_id)) = s.split_once(DELIMITER) {
            Self { tool_call_id, model_call_id: Some(model_call_id) }
        } else {
            Self { tool_call_id: s, model_call_id: None }
        }
    }
    pub fn format(tool_call_id: ByteStr, model_call_id: Option<ByteStr>) -> ByteStr {
        if let Some(model_call_id) = model_call_id {
            format!("{tool_call_id}{DELIMITER}{model_call_id}").into()
        } else {
            tool_call_id
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tool_call_id)?;
        if let Some(ref model_call_id) = self.model_call_id {
            f.write_str(DELIMITER)?;
            f.write_str(model_call_id)?;
        }
        Ok(())
    }
}
