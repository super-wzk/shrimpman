use mhf_resource::{
    action_definition::{self, Definition},
    container::open_layers,
    dat::Dat,
};

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mhfdat.bin only"]
fn original_dat_action_definitions_preserve_all_three_record_tables() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let opened = open_layers(&source, usize::MAX, 16).unwrap();
    let file = Dat::parse(opened.payload()).unwrap();
    let bytes = file.as_bytes();
    let mut actions = 0;
    let mut steps = 0;
    let mut transitions = 0;
    let mut events = 0;
    for weapon in 0..action_definition::WEAPON_COUNT {
        let directory = action_definition::weapon_actions(bytes, 0, weapon).unwrap();
        for action in 0..directory.count {
            let definition = Definition::parse(bytes, 0, weapon, action as u16).unwrap();
            for (index, step) in definition.steps.iter().enumerate() {
                assert_eq!(step.to_bytes(), bytes[definition.step_span(index).unwrap()]);
            }
            for (index, transition) in definition.transitions.iter().enumerate() {
                assert_eq!(
                    transition.to_bytes(),
                    bytes[definition.transition_span(index).unwrap()]
                );
            }
            for (index, event) in definition.events.iter().enumerate() {
                assert_eq!(
                    event.to_bytes(),
                    bytes[definition.event_span(index).unwrap()]
                );
            }
            actions += 1;
            steps += definition.steps.len();
            transitions += definition.transitions.len();
            events += definition.events.len();
        }
        println!("weapon[{weapon}]: {} actions", directory.count);
    }
    println!(
        "{} weapon directories, {actions} actions, {steps} steps, {transitions} transitions, {events} events",
        action_definition::WEAPON_COUNT
    );
}
