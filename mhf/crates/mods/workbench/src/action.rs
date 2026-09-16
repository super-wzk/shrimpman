//! Operations that validated nodes can offer. The parser records which node
//! offers one, `edit` performs it, and the UI presents it.

/// One complete operation on a validated node. It stays plain data, so a
/// pending worker request can carry it without holding code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeAction {
    /// Add the fixed-size `0xF0000` child to one FMOD object.
    InitializeRenderingBlock,
}

impl NodeAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InitializeRenderingBlock => "初始化渲染参数",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::InitializeRenderingBlock => {
                "添加固定 84 字节的 0xF0000 数据块；版本字为 0x00010000，其余 word 为 0。"
            }
        }
    }
}
