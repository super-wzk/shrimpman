use mhf_mod_sdk::abi::game::LaunchParams32;

/// Fill the ABI's temporary local character selection. Call after successfully
/// preparing the local quest; the launch target remains owned by the host.
pub fn configure_local_hunter(params: &mut LaunchParams32, name: &[u8]) -> Result<(), String> {
    if name.len() >= params.selected_character_name.len() || name.contains(&0) {
        return Err("临时猎人名称超出启动字段或包含 NUL".into());
    }
    params.selected_character_id_1 = 1;
    params.selected_character_id_2 = 1;
    params.character_ids.fill(0);
    params.character_ids[0] = 1;
    params.fixed_1d58_one = 1;
    params.fixed_200c_one = 1;
    params.selected_character_name.fill(0);
    params.selected_character_name[..name.len()].copy_from_slice(name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_startup_only_changes_character_selection() {
        let mut params = LaunchParams32::default();
        params.window_width = 1234;
        params.window_height = 567;
        configure_local_hunter(&mut params, b"Workbench").unwrap();
        assert_eq!(params.selected_character_id_1, 1);
        assert_eq!(params.selected_character_id_2, 1);
        assert_eq!(params.character_ids[0], 1);
        assert!(params.character_ids[1..].iter().all(|&id| id == 0));
        assert_eq!(&params.selected_character_name[..10], b"Workbench\0");
        assert_eq!((params.window_width, params.window_height), (1234, 567));
        let before = params.selected_character_name;
        assert!(configure_local_hunter(&mut params, b"bad\0name").is_err());
        assert_eq!(params.selected_character_name, before);
    }
}
