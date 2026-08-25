use std::ffi::CString;

use binrw::NullString;
use jiff::Timestamp;
use shrimpman_common::encoding::encode_shift_jis;
use shrimpman_discovery::{ServiceInstance, ServiceState};
use shrimpman_domain::world::World;
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::outbound::{
    LandEntry, MAX_INDEXED_ENTRIES, WorldEntry, WorldList, WorldListMetadata, WorldText,
};
use crate::{
    EntranceSessionContext, InternalError, MhfBin8, application::service_names,
    entrance_list::EntranceList,
};

pub(super) struct ListWorldsHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsHandler {
    type Inbound = ();
    type Error = InternalError;

    async fn handle(
        &self,
        context: EntranceSessionContext,
        _inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let response = execute(context)?;
        outbound.send(response).await?;
        Ok(())
    }
}

fn execute(context: EntranceSessionContext) -> Result<MhfBin8<WorldList>, InternalError> {
    let instances = context
        .service_context()
        .discovery()
        .instances(&service_names::WORLD);
    let entries = world_entries(&instances)?;

    Ok(MhfBin8::new(WorldList(EntranceList {
        entries,
        metadata: WorldListMetadata {
            server_time: Timestamp::now().into(),
            // TODO: Derive this from the guild member limit policy once it is configurable.
            max_guild_members: 60,
        },
    })))
}

fn world_entries(instances: &[ServiceInstance]) -> Result<Vec<WorldEntry>, InternalError> {
    let mut worlds = instances
        .iter()
        .filter(|instance| instance.state == ServiceState::Ready)
        .filter_map(
            |instance| match instance.decode_metadata::<World>() {
                Ok(world) => Some(world),
                Err(error) => {
                    tracing::warn!(
                        instance_id = ?instance.id,
                        %error,
                        "Ignoring invalid World service metadata"
                    );
                    None
                }
            },
        )
        .collect::<Vec<_>>();

    worlds.sort_by(|left, right| left.key.cmp(&right.key));
    worlds
        .into_iter()
        .take(MAX_INDEXED_ENTRIES)
        .enumerate()
        .map(|(index, world)| encode_world(index as u16, world))
        .collect()
}

fn encode_world(index: u16, world: World) -> Result<WorldEntry, InternalError> {
    let World {
        address,
        name,
        description,
        world_type,
        season,
        content,
        client_compatibility,
        mut lands,
        ..
    } = world;

    lands.sort_by(|left, right| left.key.cmp(&right.key));
    let lands = lands
        .into_iter()
        .take(MAX_INDEXED_ENTRIES)
        .enumerate()
        .map(|(index, land)| LandEntry {
            port: land.port,
            index: index as u16,
            max_players: land.max_players,
            current_players: land.current_players,
        })
        .collect();

    Ok(WorldEntry {
        address,
        index,
        world_type,
        season,
        content,
        text: WorldText {
            name: encode_null_string(&name)?,
            description: encode_null_string(&description)?,
        },
        client_compatibility,
        lands,
    })
}

fn encode_null_string(value: &str) -> Result<NullString, InternalError> {
    Ok(NullString(
        CString::new(encode_shift_jis(value)?)?.into_bytes(),
    ))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use shrimpman_discovery::{ServiceInstanceId, ServiceName};
    use shrimpman_domain::world::{
        ClientCompatibility, Land, LandKey, WorldContent, WorldKey, WorldSeason, WorldType,
    };

    use super::*;

    #[test]
    fn maps_ready_worlds_in_stable_domain_key_order() {
        let instances = [
            instance(ServiceState::Ready, domain_world("beta", 54_002)),
            instance(ServiceState::Draining, domain_world("ignored", 54_003)),
            ServiceInstance::new(
                ServiceInstanceId::new(),
                ServiceName::from_static("world"),
                ServiceState::Ready,
                None,
                (),
            )
            .unwrap(),
            instance(ServiceState::Ready, domain_world("alpha", 54_001)),
        ];

        let worlds = world_entries(&instances).unwrap();

        assert_eq!(worlds.len(), 2);
        assert_eq!(worlds[0].index, 0);
        assert_eq!(worlds[0].text.name.0, b"alpha");
        assert_eq!(worlds[0].lands[0].index, 0);
        assert_eq!(worlds[0].lands[0].port, 54_001);
        assert_eq!(worlds[1].index, 1);
        assert_eq!(worlds[1].text.name.0, b"beta");
    }

    #[test]
    fn orders_lands_by_key_and_encodes_world_text_as_shift_jis() {
        let mut world = domain_world("world", 54_002);
        world.name = "テスト".to_owned();
        world.lands.push(Land {
            key: LandKey::from("alpha".to_owned()),
            port: 54_001,
            max_players: 100,
            current_players: 10,
        });

        let world = encode_world(0, world).unwrap();

        assert_eq!(world.text.name.0, [0x83, 0x65, 0x83, 0x58, 0x83, 0x67]);
        assert_eq!(world.lands[0].port, 54_001);
        assert_eq!(world.lands[0].index, 0);
        assert_eq!(world.lands[1].port, 54_002);
        assert_eq!(world.lands[1].index, 1);
    }

    #[test]
    fn limits_worlds_and_lands_to_the_wire_index_capacity() {
        let instances = (0..=MAX_INDEXED_ENTRIES)
            .map(|index| {
                instance(
                    ServiceState::Ready,
                    domain_world(&format!("world-{index:02}"), 54_000 + index as u16),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            world_entries(&instances).unwrap().len(),
            MAX_INDEXED_ENTRIES
        );

        let mut world = domain_world("world", 54_000);
        world.lands = (0..=MAX_INDEXED_ENTRIES)
            .map(|index| Land {
                key: LandKey::from(format!("land-{index:02}")),
                port: 54_000 + index as u16,
                max_players: 100,
                current_players: 5,
            })
            .collect();

        assert_eq!(
            encode_world(0, world).unwrap().lands.len(),
            MAX_INDEXED_ENTRIES
        );
    }

    fn instance(state: ServiceState, world: World) -> ServiceInstance {
        ServiceInstance::new(
            ServiceInstanceId::new(),
            ServiceName::from_static("world"),
            state,
            None,
            world,
        )
        .unwrap()
    }

    fn domain_world(key: &str, port: u16) -> World {
        World {
            key: WorldKey::from(key.to_owned()),
            address: Ipv4Addr::LOCALHOST,
            name: key.to_owned(),
            description: "Description".to_owned(),
            world_type: WorldType::Free,
            season: WorldSeason::Warm,
            content: WorldContent::AllQuests,
            client_compatibility: ClientCompatibility::ALL_PLATFORMS,
            lands: vec![Land {
                key: LandKey::from("beta".to_owned()),
                port,
                max_players: 100,
                current_players: 5,
            }],
        }
    }
}
