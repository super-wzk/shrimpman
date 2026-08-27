use jiff::Timestamp;
use shrimpman_protocol::Handler;

use super::inbound::LoginRequest;
use crate::{
    application::{InternalError, WorldSessionContext},
    exchange::LandExchange,
    response::{CommandResponse, ResponseError, Value},
};

pub(super) struct LoginHandler;

impl Handler<WorldSessionContext, LandExchange> for LoginHandler {
    type Inbound = LoginRequest;
    type Error = InternalError;

    async fn handle(
        &self,
        context: WorldSessionContext,
        inbound: Self::Inbound,
        exchange: LandExchange,
    ) -> Result<(), Self::Error> {
        let now = Timestamp::now();
        let Some(account_id) = context
            .sign_sessions()
            .authenticate(inbound.session_id, &inbound.session_token, now)
            .await?
        else {
            tracing::info!(
                session_id = ?inbound.session_id,
                character_id = ?inbound.character_id,
                "Rejected World login with an invalid session"
            );
            context.disconnect();
            return Ok(());
        };

        let Some(character) = context
            .characters()
            .find_active_for_account(account_id, inbound.character_id)
            .await?
        else {
            tracing::info!(
                ?account_id,
                character_id = ?inbound.character_id,
                "Rejected World login for a character not owned by the account"
            );
            context.disconnect();
            return Ok(());
        };

        if let Err(reason) = context.bind_character(account_id, character.id) {
            tracing::info!(
                ?account_id,
                character_id = ?character.id,
                ?reason,
                "Rejected World login"
            );
            exchange
                .send_packet_and_flush(CommandResponse::failure(
                    inbound.request_handle,
                    ResponseError::ERROR,
                ))
                .await?;
            context.disconnect();
            return Ok(());
        }

        context.characters().record_sign_in(&character, now).await?;
        let server_time =
            u32::try_from(now.as_second()).map_err(|_| InternalError::ServerTimeOutOfRange)?;
        exchange
            .send_packet_and_flush(CommandResponse::success(
                inbound.request_handle,
                Value(server_time),
            ))
            .await?;
        tracing::info!(
            ?account_id,
            character_id = ?character.id,
            land = ?context.land(),
            request_version = inbound.request_version,
            "World login succeeded"
        );

        Ok(())
    }
}
