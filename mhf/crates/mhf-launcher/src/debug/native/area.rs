use super::{Runtime, State, get, put};

const EXIT_SIZE: usize = 52;
// Look a short distance beyond the requested step, before monster wall collision
// can stop the actor's center short of a hunter-sized passage.
const APPROACH_DISTANCE: f32 = 256.0;

/// 10B4BC00 reads these same 52-byte exit records. Probe before monster collision
/// and also accept a downward entrance from anywhere above its footprint, without
/// requiring movement or a particular facing. For example, quest
/// 54594's 129 -> 139 cliff has Y=-1900..-400 while the ledge is around Y=0.
pub(super) unsafe fn enter_at_position(
    state: &State,
    runtime: &Runtime,
    area: u16,
    position: [f32; 3],
    movement: [f32; 2],
) -> bool {
    let distance = movement[0].hypot(movement[1]);
    if !distance.is_finite() || !position.iter().all(|v| v.is_finite()) {
        return false;
    }
    let (direction, reach) = if distance >= 0.001 {
        (
            [movement[0] / distance, movement[1] / distance],
            distance + APPROACH_DISTANCE,
        )
    } else {
        ([0.0, 0.0], 0.0)
    };
    unsafe {
        let mut nearest: Option<(usize, f32, f32)> = None;
        for record in exit_records(state, runtime, area) {
            let flags = get::<u16>(record + 2);
            // Confirmation-based travel keeps its original interaction. The
            // native availability check also retains the map's phase restrictions.
            if flags & 0x0c00 != 0 || call_eax(state, 0x10b65fd0, record) == 0 {
                continue;
            }
            if ![36, 40, 44]
                .into_iter()
                .all(|offset| get::<f32>(record + offset).is_finite())
            {
                continue;
            }
            let origin = [get(record + 4), get(record + 8), get(record + 12)];
            let width = get::<f32>(record + 16);
            let height = get::<f32>(record + 20);
            if !origin.iter().all(|v: &f32| v.is_finite())
                || !width.is_finite()
                || width <= 0.0
                || !height.is_finite()
                || height <= 0.0
                || position[1] < origin[1]
                || !(origin[1] + height).is_finite()
            {
                continue;
            }
            // The exit's height describes its trigger volume, not the height of
            // the ledge above it. Do not use it as an upper approach limit.
            let hit = match flags & 0xff {
                0 => circle_entry(position, direction, origin, width, reach),
                1 => {
                    let end = [get(record + 24), get(record + 28), get(record + 32)];
                    rectangle_entry(position, direction, origin, end, width, reach)
                }
                _ => None,
            };
            if let Some(hit) = hit {
                let drop = (position[1] - origin[1] - height).max(0.0);
                if nearest.is_none_or(|(_, previous_hit, previous_drop)| {
                    (hit, drop) < (previous_hit, previous_drop)
                }) {
                    // Overlapping footprints choose the closest entrance below.
                    nearest = Some((record, hit, drop));
                }
            }
        }
        if let Some((record, _, _)) = nearest {
            // EAX=record. This writes the native destination, arrival position,
            // facing, fade/loading flags and hunter transition state together.
            call_eax(state, 0x10b4b9f0, record);
            return true;
        }
    }
    false
}

pub(super) unsafe fn areas(state: &State) -> Vec<u16> {
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        if scene == 0 {
            return Vec::new();
        }
        let map = usize::from(get::<u16>(scene + 52));
        if !(1..=97).contains(&map) {
            return Vec::new();
        }
        let count = usize::from(state.read::<u16>(0x11864828 + map * 2));
        let entries = state.read::<usize>(0x11a3fa28 + map * 4);
        if entries == 0 || count > 16 {
            return Vec::new();
        }
        (0..count).map(|index| get(entries + index * 2)).collect()
    }
}

pub(super) unsafe fn change(
    state: &State,
    runtime: &Runtime,
    destination: u16,
) -> Result<String, String> {
    unsafe {
        let areas = areas(state);
        if !areas.contains(&destination) {
            return Err("目标区域不属于当前地图".into());
        }
        let scene = state.read::<usize>(0x1e7fff3c);
        if get::<u16>(scene + 20) == destination {
            return Err("已经位于此区域".into());
        }
        // Prefer an authentic inbound portal, even when the current area is not
        // adjacent to it. The native loader still owns the fade and arrival.
        for source in areas {
            for record in exit_records(state, runtime, source) {
                if call_eax(state, 0x10b4c5e0, usize::from(get::<u16>(record))) as u16
                    == destination
                    && call_eax(state, 0x10b65fd0, record) != 0
                    && [36, 40, 44]
                        .into_iter()
                        .all(|offset| get::<f32>(record + offset).is_finite())
                {
                    call_eax(state, 0x10b4b9f0, record);
                    return Ok(format!("正在前往区域 {destination}"));
                }
            }
        }
        // One-way arenas may have no inbound portal back to camp. Use the native
        // area's default spawn (also used by 109EE410), not the old area's XYZ.
        let spawn = state.address(0x11869158) + usize::from(destination) * 40;
        let position = [
            get::<f32>(spawn),
            get::<f32>(spawn + 4),
            get::<f32>(spawn + 8),
        ];
        if !position.iter().all(|value| value.is_finite()) {
            return Err("目标区域没有有效出生点".into());
        }
        let mut record = [0_u8; EXIT_SIZE];
        let address = record.as_mut_ptr() as usize;
        put(address, destination);
        for (index, coordinate) in position.into_iter().enumerate() {
            put(address + 36 + index * 4, coordinate);
        }
        call_eax(state, 0x10b4b9f0, address);
        Ok(format!("正在前往区域 {destination} 的原生出生点"))
    }
}

// Returned pointers are consumed immediately on the game thread, before loading
// can replace the quest buffer. They are never stored in snapshots or UI state.
unsafe fn exit_records(state: &State, runtime: &Runtime, area: u16) -> Vec<usize> {
    unsafe {
        let quest = state.read::<usize>(0x1e8001ec);
        if quest == 0 {
            return Vec::new();
        }
        // DI=current area, including through the nested 10B4C650 lookup.
        let exits: usize;
        std::arch::asm!(
            "call ecx",
            in("ecx") state.address(0x10aa4470),
            in("edi") u32::from(area),
            lateout("eax") exits,
            clobber_abi("C"),
        );
        let buffer = state.read::<usize>(0x1ed528f4);
        let size = runtime
            .quest_override
            .as_ref()
            .unwrap_or(&state.session.quest.bytes)
            .len();
        let end = if buffer != 0 && (buffer..buffer + size).contains(&exits) {
            buffer + size
        } else if (quest + 2896..quest + 0xd40).contains(&exits) {
            quest + 0xd40
        } else {
            return Vec::new();
        };
        (0..((end - exits) / EXIT_SIZE).min(64))
            .map(|index| exits + index * EXIT_SIZE)
            .take_while(|record| get::<u16>(*record) != u16::MAX)
            .collect()
    }
}

fn circle_entry(
    position: [f32; 3],
    direction: [f32; 2],
    origin: [f32; 3],
    radius: f32,
    reach: f32,
) -> Option<f32> {
    let offset = [origin[0] - position[0], origin[2] - position[2]];
    if offset[0] * offset[0] + offset[1] * offset[1] <= radius * radius {
        return Some(0.0);
    }
    let along = offset[0] * direction[0] + offset[1] * direction[1];
    if along <= 0.0 {
        return None;
    }
    let across = offset[0] * direction[1] - offset[1] * direction[0];
    let discriminant = radius * radius - across * across;
    if discriminant < 0.0 {
        return None;
    }
    let entry = (along - discriminant.sqrt()).max(0.0);
    (entry <= reach).then_some(entry)
}

fn rectangle_entry(
    position: [f32; 3],
    direction: [f32; 2],
    start: [f32; 3],
    end: [f32; 3],
    width: f32,
    reach: f32,
) -> Option<f32> {
    if !end.iter().all(|value| value.is_finite()) {
        return None;
    }
    let edge = [end[0] - start[0], end[2] - start[2]];
    let length = edge[0].hypot(edge[1]);
    if length < 0.001 {
        return None;
    }
    let axis = [edge[0] / length, edge[1] / length];
    let offset = [position[0] - start[0], position[2] - start[2]];
    let along = offset[0] * axis[0] + offset[1] * axis[1];
    let across = offset[0] * -axis[1] + offset[1] * axis[0];
    if (0.0..=length).contains(&along) && across.abs() <= width {
        return Some(0.0);
    }
    let along_move = direction[0] * axis[0] + direction[1] * axis[1];
    let across_move = direction[0] * -axis[1] + direction[1] * axis[0];
    if (length * 0.5 - along) * along_move - across * across_move <= 0.0 {
        return None;
    }
    let mut entry = 0.0_f32;
    let mut exit = reach;
    for (position, movement, min, max) in [
        (along, along_move, 0.0, length),
        (across, across_move, -width, width),
    ] {
        if movement.abs() < 0.0001 {
            if position < min || position > max {
                return None;
            }
        } else {
            let a = (min - position) / movement;
            let b = (max - position) / movement;
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
            if entry > exit {
                return None;
            }
        }
    }
    Some(entry)
}

unsafe fn call_eax(state: &State, address: usize, argument: usize) -> usize {
    let result;
    unsafe {
        std::arch::asm!(
            "call ecx",
            in("ecx") state.address(address),
            inlateout("eax") argument => result,
            clobber_abi("C"),
        );
    }
    result
}
