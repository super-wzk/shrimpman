use super::*;

fn mouse_sample(size: usize, pressed: bool) -> Vec<u8> {
    let mut data = vec![0; size];
    data[..4].copy_from_slice(&12_i32.to_le_bytes());
    data[4..8].copy_from_slice(&(-8_i32).to_le_bytes());
    data[8..12].copy_from_slice(&120_i32.to_le_bytes());
    data[12] = if pressed { 0x80 } else { 0 };
    data
}

#[test]
fn ui_mouse_press_stays_hidden_after_the_window_closes() {
    for size in [16, 20] {
        let mut input = PolledInput::<8>::default();
        let mut data = mouse_sample(size, true);
        input.mouse(1, &mut data, true, |_| None);
        assert_eq!(data, vec![0; size]);

        for _ in 0..3 {
            data = mouse_sample(size, true);
            input.mouse(1, &mut data, false, |_| None);
            assert_eq!(
                data,
                vec![0; size],
                "closing the UI cannot leak a held press"
            );
        }

        data = mouse_sample(size, false);
        input.mouse(1, &mut data, false, |_| None);
        assert_eq!(data, mouse_sample(size, false));
        data = mouse_sample(size, true);
        input.mouse(1, &mut data, false, |_| None);
        assert_eq!(
            data,
            mouse_sample(size, true),
            "the next click belongs to the game"
        );
    }
}

#[test]
fn game_drag_keeps_its_state_until_release_after_a_modal_opens() {
    let mut input = PolledInput::<8>::default();
    let mut data = mouse_sample(16, true);
    input.mouse(1, &mut data, false, |_| None);
    for _ in 0..3 {
        data = mouse_sample(16, true);
        input.mouse(1, &mut data, true, |_| None);
        assert_eq!(
            data,
            mouse_sample(16, true),
            "do not synthesize a game release"
        );
    }
    data = mouse_sample(16, false);
    input.mouse(1, &mut data, true, |_| None);
    assert_eq!(data, vec![0; 16]);
    data = mouse_sample(16, true);
    input.mouse(1, &mut data, true, |_| None);
    assert_eq!(data, vec![0; 16]);
}

#[test]
fn keyboard_ownership_is_per_scan_code_and_survives_policy_changes() {
    let mut input = PolledInput::<256>::default();
    let mut physical = [0; 256];
    physical[0x1e] = 0x80; // A starts in the game.
    let mut data = physical;
    input.keyboard(1, &mut data, false, |_| None);
    assert_eq!(data, physical);

    physical[0x30] = 0x80; // B starts in the UI.
    data = physical;
    input.keyboard(1, &mut data, true, |_| None);
    assert_eq!(data[0x1e], 0x80);
    assert_eq!(data[0x30], 0);

    physical[0x2e] = 0x80; // C starts after the UI closes.
    data = physical;
    input.keyboard(1, &mut data, false, |_| None);
    assert_eq!(data[0x1e], 0x80);
    assert_eq!(data[0x30], 0);
    assert_eq!(data[0x2e], 0x80);

    physical[0x30] = 0;
    data = physical;
    input.keyboard(1, &mut data, false, |_| None);
    assert_eq!(data, physical);
    physical[0x30] = 0x80;
    data = physical;
    input.keyboard(1, &mut data, false, |_| None);
    assert_eq!(data, physical);
}

#[test]
fn wheel_and_extra_buttons_obey_capture_and_passthrough() {
    let mut input = PolledInput::<8>::default();
    let mut physical = mouse_sample(20, false);
    let mut data = physical.clone();
    input.mouse(1, &mut data, true, |_| None);
    assert_eq!(data, vec![0; 20]);
    physical[19] = 0x80;
    data = physical.clone();
    input.mouse(1, &mut data, true, |_| None);
    assert_eq!(data, vec![0; 20]);
    data = physical;
    input.mouse(1, &mut data, false, |_| None);
    assert_eq!(data, vec![0; 20]);

    // Switching back to the four-button format discards absent buttons.
    data = mouse_sample(16, false);
    input.mouse(1, &mut data, false, |_| None);
    assert_eq!(data, mouse_sample(16, false));
}

#[test]
fn unknown_formats_are_untouched_and_recreated_devices_start_fresh() {
    let mut mouse = PolledInput::<8>::default();
    let mut keyboard = PolledInput::<256>::default();
    for size in [0, 12, 17, 80, 272] {
        let mut data = vec![0xa5; size];
        mouse.mouse(1, &mut data, true, |_| None);
        keyboard.keyboard(1, &mut data, true, |_| None);
        assert_eq!(data, vec![0xa5; size]);
    }
    let mut data = mouse_sample(16, true);
    mouse.mouse(1, &mut data, true, |_| None);
    data = mouse_sample(16, true);
    mouse.mouse(2, &mut data, false, |_| None);
    assert_eq!(data, mouse_sample(16, true));
}

#[test]
fn native_press_ownership_precedes_the_first_device_sample() {
    let mut mouse = PolledInput::<8>::default();
    let mut data = mouse_sample(16, true);
    // WM_LBUTTONDOWN occurred over UI; the cursor left before DirectInput polled.
    mouse.mouse(1, &mut data, false, |button| (button == 0).then_some(true));
    assert_eq!(data, vec![0; 16]);
    data = mouse_sample(16, true);
    mouse.mouse(1, &mut data, false, |_| None);
    assert_eq!(data, vec![0; 16]);

    let mut keyboard = PolledInput::<256>::default();
    let mut data = [0; 256];
    data[0x1e] = 0x80;
    // A game keydown preceded opening a modal, before the first device sample.
    keyboard.keyboard(1, &mut data, true, |code| (code == 0x1e).then_some(false));
    assert_eq!(data[0x1e], 0x80);
}
