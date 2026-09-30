use super::{Document, Kind};
use mhf_resource::{action_definition::AttackDirectory, sdt::Sdt};

impl Document {
    /// Build only lookup metadata from the actual decoded SDT data layer.
    pub fn parsed_attack_directory(&self) -> Result<AttackDirectory, String> {
        let node = self
            .nodes
            .iter()
            .position(|node| node.kind == Kind::Sdt)
            .ok_or("资源中没有 SDT 数据层")?;
        let bytes = self.bytes(node).ok_or("SDT 数据层范围无效")?;
        let file = Sdt::parse(bytes).map_err(|error| error.to_string())?;
        AttackDirectory::from_sdt("mhfsdt.bin", &file).map_err(|error| error.to_string())
    }
}
