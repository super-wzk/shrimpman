//! Native source vertex packing, before the existing FVF/GPU conversion.

use crate::mesh;
use std::mem::MaybeUninit;

pub(crate) struct Vertices<'a> {
    pub format: u32,
    pub byte_size: u32,
    positions: &'a [u8],
    normals: &'a [u8],
    colors: &'a [u8],
    uvs: Option<&'a [u8]>,
    weights: Option<&'a [u8]>,
    attribute: Option<&'a [u8]>,
}

fn array(bytes: Option<&[u8]>, count: usize, stride: usize) -> Result<&[u8], String> {
    bytes
        .ok_or("missing FMOD vertex attribute")?
        .get(..count * stride)
        .ok_or_else(|| "truncated FMOD vertex attribute".into())
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

// The native x87 colour conversion truncates to i64, then uses its low DWORD.
// Integer-indefinite (NaN/overflow) has a zero low DWORD. Ordinary colours fit
// the i32 fast path, avoiding the i686 i64 conversion helper.
fn color_integer(value: f32) -> u32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        value as i32 as u32
    } else if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
        value as i64 as u32
    } else {
        0
    }
}

fn weight_integer(value: f32) -> u8 {
    // CVTTSS2SI returns integer-indefinite outside this range; its low BYTE is 0.
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        value as i32 as u8
    } else {
        0
    }
}

impl<'a> Vertices<'a> {
    pub fn new(
        count: u32,
        positions: &'a [u8],
        normals: Option<&'a [u8]>,
        colors: Option<&'a [u8]>,
        uvs: Option<&'a [u8]>,
        weights: Option<&'a [u8]>,
        attribute: Option<&'a [u8]>,
    ) -> Result<Self, String> {
        let skinned = weights.is_some();
        if !skinned && attribute.is_some() {
            // 10003190's allocation and emitted stride disagree. Do not fall
            // back to that converter, whose untextured path overwrites its heap.
            return Err("unsupported FMOD unweighted 0x120000 vertex layout".into());
        }
        if skinned && uvs.is_none() {
            return Err("FMOD weighted vertices require UVs".into());
        }
        // Keep the native source allocation size, including the unused tail in
        // the untextured 28-byte layout. This also bounds all attribute products.
        let byte_size = mesh::byte_size(count as usize, if skinned { 60 } else { 36 })?;
        let count = count as usize;
        Ok(Self {
            format: if skinned {
                0x4135
            } else if uvs.is_some() {
                0x35
            } else {
                0x25
            },
            byte_size,
            positions: array(Some(positions), count, 12)?,
            normals: array(normals, count, 12)?,
            colors: array(colors, count, 16)?,
            uvs: uvs.map(|bytes| array(Some(bytes), count, 8)).transpose()?,
            weights,
            attribute: attribute
                .map(|bytes| array(Some(bytes), count, 16))
                .transpose()?,
        })
    }

    pub fn skinned(&self) -> bool {
        self.weights.is_some()
    }

    /// Initialize `byte_size / 4` words in the final CRT allocation. On error
    /// the caller still owns that allocation and must not publish its contents.
    pub fn encode_into(&self, output: &mut [MaybeUninit<u32>]) -> Result<(), String> {
        let skinned = self.skinned();
        let stride = if skinned {
            15
        } else if self.uvs.is_some() {
            9
        } else {
            7
        };
        let count = self.positions.len() / 12;
        let (vertices, padding) = output.split_at_mut(count * stride);
        // Native untextured sources leave this tail unused. Initialize it before
        // build_model creates a Rust byte slice covering the whole allocation.
        padding.fill(MaybeUninit::new(0));
        let mut weights = self.weights.unwrap_or_default();
        for (i, vertex) in vertices.chunks_exact_mut(stride).enumerate() {
            for axis in 0..3 {
                vertex[axis].write(word(self.positions, i * 12 + axis * 4));
                vertex[axis + 3].write(word(self.normals, i * 12 + axis * 4));
            }
            let color: [u32; 4] = std::array::from_fn(|channel| {
                color_integer(f32::from_bits(word(self.colors, i * 16 + channel * 4)))
            });
            vertex[6].write(color[3] << 24 | color[0] << 16 | color[1] << 8 | color[2]);
            if let Some(uvs) = self.uvs {
                vertex[7].write(word(uvs, i * 8));
                vertex[8].write(word(uvs, i * 8 + 4));
            }
            if skinned {
                for (axis, value) in vertex[9..13].iter_mut().enumerate() {
                    value.write(
                        self.attribute
                            .map_or(0, |bytes| word(bytes, i * 16 + axis * 4)),
                    );
                }
                let header = weights.get(..4).ok_or("truncated FMOD weight count")?;
                let count = word(header, 0) as usize;
                if count > 4 {
                    return Err("FMOD vertex exceeds four native bone influences".into());
                }
                let data = weights
                    .get(4..4 + count * 8)
                    .ok_or("truncated FMOD weights")?;
                weights = &weights[4 + count * 8..];
                let mut bones = [0u8; 4];
                let mut amounts = [0i32; 4];
                let mut last = 0;
                for (influence, entry) in data.as_chunks::<8>().0.iter().enumerate() {
                    bones[influence] = word(entry, 0) as u8;
                    let weight = f32::from_bits(word(entry, 4));
                    amounts[influence] = i32::from(weight_integer((weight * 255.0) * 0.01));
                    if amounts[influence] != 0 {
                        last = influence;
                    }
                }
                amounts[last] += 255 - amounts.iter().sum::<i32>();
                // Preserve the native single forward pass over zero weights.
                for influence in 0..count.saturating_sub(1) {
                    if amounts[influence] == 0 {
                        amounts[influence] = amounts[influence + 1];
                        bones[influence] = bones[influence + 1];
                        amounts[influence + 1] = 0;
                    }
                }
                vertex[13].write(u32::from_be_bytes(bones));
                vertex[14].write(
                    (amounts[0] as u32) << 24
                        | (amounts[1] as u32) << 16
                        | (amounts[2] as u32) << 8
                        | amounts[3] as u32,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_conversion_keeps_native_truncation_and_indefinite_bits() {
        for (value, expected) in [
            (255.9, 255),
            (-1.75, u32::MAX),
            (2_147_483_648.0, 0x8000_0000),
            (4_294_967_808.0, 512),
            (9_223_372_036_854_775_808.0, 0),
            (f32::NAN, 0),
            (f32::INFINITY, 0),
            (f32::NEG_INFINITY, 0),
        ] {
            assert_eq!(color_integer(value), expected);
        }
    }

    #[test]
    fn incomplete_and_unsupported_attributes_cannot_produce_a_source() {
        let positions = [0; 12];
        let colors = [0; 16];
        let uvs = [0; 8];
        for (normals, colors, weights, attribute) in [
            (None, colors.as_slice(), None, None),
            (Some(positions.as_slice()), &colors[..15], None, None),
            (
                Some(positions.as_slice()),
                colors.as_slice(),
                None,
                Some(colors.as_slice()),
            ),
            (
                Some(positions.as_slice()),
                colors.as_slice(),
                Some(&[][..]),
                None,
            ),
        ] {
            assert!(
                Vertices::new(
                    1,
                    &positions,
                    normals,
                    Some(colors),
                    None,
                    weights,
                    attribute
                )
                .is_err()
            );
        }
        for weights in [&[][..], &[1, 0, 0, 0][..], &[5, 0, 0, 0][..]] {
            let vertices = Vertices::new(
                1,
                &positions,
                Some(&positions),
                Some(&colors),
                Some(&uvs),
                Some(weights),
                None,
            )
            .unwrap();
            assert!(
                vertices
                    .encode_into(&mut [MaybeUninit::uninit(); 15])
                    .is_err()
            );
        }
    }
}
