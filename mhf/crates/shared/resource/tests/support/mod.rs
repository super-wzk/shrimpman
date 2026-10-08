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

pub fn dat_image(size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..4].copy_from_slice(b"mhf\x1a");
    dword(&mut bytes, 4, 89);
    dword(&mut bytes, 12, 3016);
    bytes
}

pub fn simple_archive(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; 4 + entries.len() * 8];
    dword(&mut bytes, 0, entries.len() as u32);
    for (index, payload) in entries.iter().enumerate() {
        let offset = bytes.len() as u32;
        dword(&mut bytes, 4 + index * 8, offset);
        dword(&mut bytes, 8 + index * 8, payload.len() as u32);
        bytes.extend_from_slice(payload);
    }
    bytes
}
