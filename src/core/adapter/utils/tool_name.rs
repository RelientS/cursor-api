use byte_str::ByteStr;

pub struct ToolName {
    pub tool_name: ByteStr,
    pub name: ByteStr,
    pub server_name: ByteStr,
}

impl ToolName {
    pub fn parse(s: ByteStr) -> Self {
        let tool_name: ByteStr;
        let server_len;
        if let Some(s) = s.strip_prefix("mcp__") {
            if let Some((server, tool)) = s.split_once("__") {
                server_len = server.len();
                tool_name = format!("mcp_{server}_{tool}").into();
            } else {
                server_len = 6;
                tool_name = format!("mcp_custom_{s}").into();
            }
        } else {
            server_len = 6;
            tool_name = format!("mcp_custom_{s}").into();
        }
        let name;
        let server_name;
        unsafe {
            let d = server_len.unchecked_add(4);
            server_name = tool_name.slice_unchecked(4..d);
            name = tool_name.slice_unchecked(d.unchecked_add(1)..);
        }
        Self { tool_name, name, server_name }
    }
}
