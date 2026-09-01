use shrimpman_domain::character::CharacterId;
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::{
    super::list_worlds::build_world_list,
    inbound::ListWorldsPresence,
    outbound::{CharacterPresence, CharacterPresenceList, ListWorldsPresenceResponse},
};
use crate::{EntranceSessionContext, InternalError, MhfBin8};

pub(super) struct ListWorldsPresenceHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsPresenceHandler {
    type Inbound = ListWorldsPresence;
    type Error = InternalError;

    async fn handle(
        &self,
        context: EntranceSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let worlds = build_world_list(&context)?;

        match inbound.character_ids {
            None => outbound.send(worlds).await?,
            Some(character_ids) => {
                outbound
                    .send(ListWorldsPresenceResponse {
                        worlds,
                        presences: MhfBin8::new(unknown_character_presences(character_ids)),
                    })
                    .await?;
            }
        }

        Ok(())
    }
}

fn unknown_character_presences(character_ids: Vec<CharacterId>) -> CharacterPresenceList {
    // Entrance has no distributed live-session snapshot yet. Preserve the
    // request order and report each location as unknown.
    CharacterPresenceList(
        character_ids
            .into_iter()
            .map(|_| CharacterPresence {
                world_land_indices: None,
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_requested_character_count_as_unknown_presences() {
        let presences = unknown_character_presences(vec![
            CharacterId::from(42),
            CharacterId::from(100),
        ]);

        assert_eq!(presences.0.entries.len(), 2);
        assert!(
            presences
                .0
                .entries
                .iter()
                .all(|presence| presence.world_land_indices.is_none())
        );
    }
}
