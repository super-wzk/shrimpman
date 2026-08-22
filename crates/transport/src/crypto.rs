use crate::{
    error::TransportError,
    frame::{CryptHeader, PacketChecksums},
};

const INITIAL_ROTATION_KEY: u32 = 995_117;
const OUTBOUND_ROTATION_DELTA: u8 = 3;

const ENCRYPT_KEY: [u8; 256] = [
    0x90, 0x51, 0x26, 0x25, 0x04, 0xBF, 0xCF, 0x4C, 0x92, 0x02, 0x52, 0x7A, 0x70, 0x1A, 0x41, 0x88,
    0x8C, 0xC2, 0xCE, 0xB8, 0xF6, 0x57, 0x7E, 0xBA, 0x83, 0x63, 0x2C, 0x24, 0x9A, 0x67, 0x86, 0x0C,
    0xBE, 0x72, 0xFD, 0xB6, 0x7B, 0x79, 0xB0, 0x22, 0x5A, 0x60, 0x5C, 0x4F, 0x49, 0xE2, 0x0E, 0xF5,
    0x3A, 0x81, 0xAE, 0x11, 0x6B, 0xF0, 0xA1, 0x01, 0xE8, 0x65, 0x8D, 0x5B, 0xDC, 0xCC, 0x93, 0x18,
    0xB3, 0xAB, 0x77, 0xF7, 0x8E, 0xEC, 0xEF, 0x05, 0x00, 0xCA, 0x4E, 0xA7, 0xBC, 0xB5, 0x10, 0xC6,
    0x6C, 0xC0, 0xC4, 0xE5, 0x87, 0x3F, 0xC1, 0x82, 0x29, 0x96, 0x45, 0x73, 0x07, 0xCB, 0x43, 0xF9,
    0xF3, 0x08, 0x89, 0xD0, 0x99, 0x6A, 0x3B, 0x37, 0x19, 0xD4, 0x40, 0xEA, 0xD7, 0x85, 0x16, 0x66,
    0x1E, 0x9C, 0x39, 0xBB, 0xEE, 0x4A, 0x03, 0x8A, 0x36, 0x2D, 0x13, 0x1D, 0x56, 0x48, 0xC7, 0x0D,
    0x59, 0xB2, 0x44, 0xA3, 0xFE, 0x8B, 0x32, 0x1B, 0x84, 0xA0, 0x2E, 0x62, 0x17, 0x42, 0xB9, 0x9B,
    0x2B, 0x75, 0xD8, 0x1C, 0x3C, 0x4D, 0x76, 0x27, 0x6E, 0x28, 0xD3, 0x33, 0xC3, 0x21, 0xAF, 0x34,
    0x23, 0xDD, 0x68, 0x9F, 0xF1, 0xAD, 0xE1, 0xB4, 0xE7, 0xA6, 0x74, 0x15, 0x4B, 0xFA, 0x3D, 0x5F,
    0x7C, 0xDA, 0x2F, 0x0A, 0xE3, 0x7D, 0xC8, 0xB7, 0x12, 0x6F, 0x9E, 0xA9, 0x14, 0x53, 0x97, 0x8F,
    0x64, 0xF4, 0xF8, 0xA2, 0xA4, 0x2A, 0xD2, 0x47, 0x9D, 0x71, 0xC5, 0xE9, 0x06, 0x98, 0x20, 0x54,
    0x80, 0xAA, 0xF2, 0xAC, 0x50, 0xD6, 0x7F, 0xD9, 0xC9, 0xCD, 0x69, 0x46, 0x6D, 0x30, 0xB1, 0x58,
    0x0B, 0x55, 0xD1, 0x5D, 0xD5, 0xBD, 0x31, 0xDE, 0xA5, 0xE4, 0x91, 0x0F, 0x61, 0x38, 0xDF, 0xA8,
    0xE6, 0x3E, 0x1F, 0x35, 0xED, 0xDB, 0x94, 0xEB, 0x09, 0x5E, 0x95, 0xFB, 0xFC, 0xE0, 0x78, 0xFF,
];

const fn invert_s_box(table: &[u8; 256]) -> [u8; 256] {
    let mut inverse = [0; 256];
    let mut index = 0;
    while index < table.len() {
        inverse[table[index] as usize] = index as u8;
        index += 1;
    }
    inverse
}

const DECRYPT_KEY: [u8; 256] = invert_s_box(&ENCRYPT_KEY);

const SHARED_KEY: [u8; 256] = [
    0xDD, 0xA8, 0x5F, 0x1E, 0x57, 0xAF, 0xC0, 0xCC, 0x43, 0x35, 0x8F, 0xBB, 0x6F, 0xE6, 0xA1, 0xD6,
    0x60, 0xB9, 0x1A, 0xAE, 0x20, 0x49, 0x24, 0x81, 0x21, 0xFE, 0x86, 0x2B, 0x98, 0xB7, 0xB3, 0xD2,
    0x91, 0x01, 0x3A, 0x4C, 0x65, 0x92, 0x1C, 0xF4, 0xBE, 0xDD, 0xD9, 0x08, 0xE6, 0x81, 0x98, 0x1B,
    0x8D, 0x60, 0xF3, 0x6F, 0xA1, 0x47, 0x24, 0xF1, 0x53, 0x45, 0xC8, 0x7B, 0x88, 0x80, 0x4E, 0x36,
    0xC3, 0x0D, 0xC9, 0xD6, 0x8B, 0x08, 0x19, 0x0B, 0xA5, 0xC1, 0x11, 0x4C, 0x60, 0xF8, 0x5D, 0xFC,
    0x15, 0x68, 0x7E, 0x32, 0xC0, 0x50, 0xAB, 0x64, 0x1F, 0x8A, 0xD4, 0x08, 0x39, 0x7F, 0xC2, 0xFB,
    0xBA, 0x6C, 0xF0, 0xE6, 0xB0, 0x31, 0x10, 0xC1, 0xBF, 0x75, 0x43, 0xBB, 0x18, 0x04, 0x0D, 0xD1,
    0x97, 0xF7, 0x23, 0x21, 0x83, 0x8B, 0xCA, 0x25, 0x2B, 0xA3, 0x03, 0x13, 0xEA, 0xAE, 0xFE, 0xF0,
    0xEB, 0xFD, 0x85, 0x57, 0x53, 0x65, 0x41, 0x2A, 0x40, 0x99, 0xC0, 0x94, 0x65, 0x7E, 0x7C, 0x93,
    0x82, 0xB0, 0xB3, 0xE5, 0xC0, 0x21, 0x09, 0x84, 0xD5, 0xEF, 0x9F, 0xD1, 0x7E, 0xDC, 0x4D, 0xF5,
    0x7E, 0xCD, 0x45, 0x3C, 0x7F, 0xF5, 0x59, 0x98, 0xC6, 0x55, 0xFC, 0x9F, 0xA3, 0xB7, 0x74, 0xEE,
    0x31, 0x98, 0xE6, 0xB7, 0xBE, 0x26, 0xF4, 0x3C, 0x76, 0xF1, 0x23, 0x7E, 0x02, 0x4E, 0x3C, 0xD1,
    0xC7, 0x28, 0x23, 0x73, 0xC4, 0xD9, 0x5E, 0x0D, 0xA1, 0x80, 0xA5, 0xAA, 0x26, 0x0A, 0xA3, 0x44,
    0x82, 0x74, 0xE6, 0x3C, 0x44, 0x27, 0x51, 0x0D, 0x5F, 0xC7, 0x9C, 0xD6, 0x63, 0x67, 0xA5, 0x27,
    0x97, 0x38, 0xFB, 0x2D, 0xD3, 0xD6, 0x60, 0x25, 0x83, 0x4D, 0x37, 0x5B, 0x40, 0x59, 0x11, 0x77,
    0x51, 0x11, 0x14, 0x18, 0x07, 0x63, 0xB1, 0x34, 0x3D, 0xB8, 0x60, 0x13, 0xC2, 0xE8, 0x13, 0x82,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct CryptoOutput {
    data: Vec<u8>,
    combined_check: u16,
    checksums: PacketChecksums,
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Encrypt,
    Decrypt,
}

fn crypt(data: &[u8], rotation_key: u32, direction: Direction) -> CryptoOutput {
    let truncated_key = (((rotation_key >> 1) % 999_983) & 0xff) as u8;
    let mut derived_key = (data.len() as u32).wrapping_mul(u32::from(truncated_key) + 1);
    let mut shared_index = 1_u8;
    let mut accumulator0 = 0_u32;
    let mut accumulator1 = 0_u32;
    let mut accumulator2 = 0_u32;
    let mut output = Vec::with_capacity(data.len());

    match direction {
        Direction::Encrypt => {
            for (index, &plain) in data.iter().enumerate() {
                let key_index = ((derived_key >> 10) ^ u32::from(plain)) as u8;
                derived_key = derived_key.wrapping_mul(1277).wrapping_add(1277);
                let key_byte = ENCRYPT_KEY[usize::from(key_index)];

                accumulator2 = accumulator2
                    .wrapping_add(u32::from(shared_index).wrapping_mul(u32::from(plain)));
                accumulator1 = accumulator1.wrapping_add(u32::from(key_index));
                accumulator0 = accumulator0.wrapping_add(u32::from(key_byte) << (index & 7));

                output.push(SHARED_KEY[usize::from(shared_index)] ^ key_byte);
                shared_index = plain;
            }
        }
        Direction::Decrypt => {
            for (index, &encrypted) in data.iter().enumerate() {
                let previous_shared_index = shared_index;
                let key_index = encrypted ^ SHARED_KEY[usize::from(shared_index)];
                let key_byte = DECRYPT_KEY[usize::from(key_index)];
                shared_index = ((derived_key >> 10) ^ u32::from(key_byte)) as u8;

                accumulator0 = accumulator0.wrapping_add(u32::from(key_index) << (index & 7));
                accumulator1 = accumulator1.wrapping_add(u32::from(key_byte));
                accumulator2 = accumulator2.wrapping_add(
                    u32::from(previous_shared_index).wrapping_mul(u32::from(shared_index)),
                );

                output.push(shared_index);
                derived_key = derived_key.wrapping_mul(1277).wrapping_add(1277);
            }
        }
    }

    let combined_check = accumulator1
        .wrapping_add(accumulator0 >> 1)
        .wrapping_add(accumulator2 >> 2) as u16;
    let checksums = PacketChecksums::new(
        (accumulator0 ^ (accumulator0 >> 16)) as u16,
        (accumulator1 ^ (accumulator1 >> 16)) as u16,
        (accumulator2 ^ (accumulator2 >> 16)) as u16,
    );

    CryptoOutput {
        data: output,
        combined_check,
        checksums,
    }
}

const fn rotate_key(rotation_key: u32, delta: u8) -> u32 {
    if delta == 0 {
        rotation_key
    } else {
        (delta as u32).wrapping_mul(rotation_key.wrapping_add(1))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DecryptState {
    rotation_key: u32,
    previous_combined_check: u16,
}

impl Default for DecryptState {
    fn default() -> Self {
        Self {
            rotation_key: INITIAL_ROTATION_KEY,
            previous_combined_check: 0,
        }
    }
}

impl DecryptState {
    pub(crate) fn decrypt(
        &mut self,
        header: CryptHeader,
        body: &[u8],
    ) -> Result<Vec<u8>, TransportError> {
        let next_key = rotate_key(self.rotation_key, header.key_rotation_delta());
        let decrypted = crypt(body, next_key, Direction::Decrypt);
        let expected = header.checksums();

        if decrypted.checksums != expected {
            return Err(TransportError::ChecksumMismatch {
                expected,
                actual: decrypted.checksums,
            });
        }

        self.rotation_key = next_key;
        self.previous_combined_check = decrypted.combined_check;
        Ok(decrypted.data)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EncryptState {
    rotation_key: u32,
    next_packet_number: u16,
    previous_combined_check: u16,
}

impl Default for EncryptState {
    fn default() -> Self {
        Self {
            rotation_key: INITIAL_ROTATION_KEY,
            next_packet_number: 0,
            previous_combined_check: 0,
        }
    }
}

impl EncryptState {
    pub(crate) fn encrypt(&mut self, payload: &[u8]) -> EncryptedBody {
        let rotation_key = rotate_key(self.rotation_key, OUTBOUND_ROTATION_DELTA);
        let encrypted = crypt(payload, rotation_key, Direction::Encrypt);
        let body = EncryptedBody {
            data: encrypted.data,
            key_rotation_delta: OUTBOUND_ROTATION_DELTA,
            packet_number: self.next_packet_number,
            previous_combined_check: self.previous_combined_check,
            checksums: encrypted.checksums,
        };

        self.rotation_key = rotation_key;
        self.next_packet_number = self.next_packet_number.wrapping_add(1);
        self.previous_combined_check = encrypted.combined_check;
        body
    }
}

pub(crate) struct EncryptedBody {
    pub(crate) data: Vec<u8>,
    pub(crate) key_rotation_delta: u8,
    pub(crate) packet_number: u16,
    pub(crate) previous_combined_check: u16,
    pub(crate) checksums: PacketChecksums,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Vector {
        key: u32,
        encrypted: [u8; 4],
        combined_check: u16,
        checksums: PacketChecksums,
    }

    const PLAIN: &[u8] = b"test";
    const VECTORS: &[Vector] = &[
        Vector {
            key: 0,
            encrypted: [0x46, 0x53, 0x28, 0x5e],
            combined_check: 0x2976,
            checksums: PacketChecksums::new(0x06ea, 0x0215, 0x8fb3),
        },
        Vector {
            key: 3,
            encrypted: [0x46, 0x95, 0x88, 0xea],
            combined_check: 0x2ae4,
            checksums: PacketChecksums::new(0x0a56, 0x01cd, 0x8fb3),
        },
        Vector {
            key: u32::MAX,
            encrypted: [0x46, 0xb5, 0xdc, 0xb2],
            combined_check: 0x2add,
            checksums: PacketChecksums::new(0x09a6, 0x021e, 0x8fb3),
        },
    ];

    #[test]
    fn matches_reference_encryption_vectors() {
        for vector in VECTORS {
            let output = crypt(PLAIN, vector.key, Direction::Encrypt);
            assert_eq!(output.data, vector.encrypted);
            assert_eq!(output.combined_check, vector.combined_check);
            assert_eq!(output.checksums, vector.checksums);
        }
    }

    #[test]
    fn matches_reference_decryption_vectors() {
        for vector in VECTORS {
            let output = crypt(&vector.encrypted, vector.key, Direction::Decrypt);
            assert_eq!(output.data, PLAIN);
            assert_eq!(output.combined_check, vector.combined_check);
            assert_eq!(output.checksums, vector.checksums);
        }
    }

    #[test]
    fn chains_packet_numbers_and_previous_checks() {
        let mut state = EncryptState::default();

        let first = state.encrypt(b"one");
        let second = state.encrypt(b"two");

        assert_eq!(first.packet_number, 0);
        assert_eq!(first.previous_combined_check, 0);
        assert_eq!(second.packet_number, 1);
        assert_ne!(second.previous_combined_check, 0);
    }
}
