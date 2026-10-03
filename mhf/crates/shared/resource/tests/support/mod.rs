pub fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = words(&[kind, count, (12 + payload.len()) as u32]);
    bytes.extend_from_slice(payload);
    bytes
}

pub fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn dword(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
