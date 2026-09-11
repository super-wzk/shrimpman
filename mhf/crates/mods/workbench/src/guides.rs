use crate::preview::{Camera, PreviewOptions};

pub(crate) const AXIS_COLORS: [[u8; 3]; 3] = [[225, 65, 65], [55, 180, 85], [65, 125, 235]];

#[repr(C)]
pub(crate) struct Vertex {
    pub position: [f32; 3],
    pub color: u32,
}

pub(crate) fn grid_spacing(camera: Camera) -> f32 {
    let desired = (camera.distance() * 0.1).clamp(0.01, 10_000.0);
    let base = 10.0_f32.powf(desired.log10().floor());
    let scale = desired / base;
    base * if scale <= 1.0 {
        1.0
    } else if scale <= 2.0 {
        2.0
    } else if scale <= 5.0 {
        5.0
    } else {
        10.0
    }
}

/// World-space lines on the Y=0 plane. The grid follows the focus region but
/// remains aligned to world coordinates, including its every-fifth major lines.
pub(crate) fn vertices(camera: Camera, options: PreviewOptions) -> Vec<Vertex> {
    let spacing = grid_spacing(camera);
    let extent = spacing * 20.0;
    let center =
        [camera.target[0], camera.target[2]].map(|value| (value / spacing).round() * spacing);
    let [r, g, b] = options.background_color.map(u32::from);
    let contrast = if 299 * r + 587 * g + 114 * b >= 128_000 {
        0
    } else {
        255
    };
    let grid_colors = [18, 35].map(|weight| {
        options
            .background_color
            .map(|value| ((u32::from(value) * (100 - weight) + contrast * weight) / 100) as u8)
    });
    let grid_color = |coordinate: f32| {
        let major = (coordinate / spacing).round() as i64 % 5 == 0;
        grid_colors[usize::from(major)]
    };
    let mut vertices = Vec::with_capacity(170);
    let mut line = |from, to, [r, g, b]: [u8; 3]| {
        let color = u32::from_be_bytes([255, r, g, b]);
        vertices.extend([
            Vertex {
                position: from,
                color,
            },
            Vertex {
                position: to,
                color,
            },
        ]);
    };
    if options.show_grid {
        for index in -20..=20 {
            let x = center[0] + index as f32 * spacing;
            let z = center[1] + index as f32 * spacing;
            if !options.show_axes || x != 0.0 {
                line(
                    [x, 0.0, center[1] - extent],
                    [x, 0.0, center[1] + extent],
                    grid_color(x),
                );
            }
            if !options.show_axes || z != 0.0 {
                line(
                    [center[0] - extent, 0.0, z],
                    [center[0] + extent, 0.0, z],
                    grid_color(z),
                );
            }
        }
    }
    if options.show_axes {
        line(
            [center[0] - extent, 0.0, 0.0],
            [center[0] + extent, 0.0, 0.0],
            AXIS_COLORS[0],
        );
        line([0.0, -extent, 0.0], [0.0, extent, 0.0], AXIS_COLORS[1]);
        line(
            [0.0, 0.0, center[1] - extent],
            [0.0, 0.0, center[1] + extent],
            AXIS_COLORS[2],
        );
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(distance: f32) -> Camera {
        Camera {
            eye: [0.0, 0.0, distance],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov_y: std::f32::consts::FRAC_PI_3,
            aspect: 1.0,
        }
    }

    #[test]
    fn grid_scales_with_zoom_and_stays_on_world_ground() {
        let options = PreviewOptions {
            show_axes: false,
            ..PreviewOptions::default()
        };
        for distance in [1.0, 350.0, 100_000.0] {
            let mut camera = camera(distance);
            camera.target = [123.0, 456.0, -789.0];
            camera.eye = [123.0, 456.0, -789.0 + distance];
            let spacing = grid_spacing(camera);
            assert!(spacing >= distance * 0.1 * 0.999 && spacing <= distance * 0.25);
            let vertices = vertices(camera, options);
            assert_eq!(vertices.len(), 164);
            for pair in vertices.as_chunks::<2>().0 {
                assert!(pair.iter().all(|vertex| vertex.position[1] == 0.0));
                let axis = if pair[0].position[0] == pair[1].position[0] {
                    0
                } else {
                    2
                };
                let cell = pair[0].position[axis] / spacing;
                assert!((cell - cell.round()).abs() < 0.002);
            }
        }
    }

    #[test]
    fn axes_and_grid_can_be_hidden_independently() {
        let options = PreviewOptions {
            show_grid: false,
            ..PreviewOptions::default()
        };
        let axes = vertices(camera(350.0), options);
        assert_eq!(axes.len(), 6);
        for (axis, pair) in axes.as_chunks::<2>().0.iter().enumerate() {
            assert!(pair[0].position[axis] < 0.0 && pair[1].position[axis] > 0.0);
            for vertex in pair {
                assert!(
                    vertex
                        .position
                        .iter()
                        .enumerate()
                        .all(|(i, &v)| i == axis || v == 0.0)
                );
            }
        }
        assert!(
            vertices(
                camera(350.0),
                PreviewOptions {
                    show_axes: false,
                    ..options
                }
            )
            .is_empty()
        );
    }
}
