use crate::config::SignEncoding;
use crate::{config, credentials::CredentialStore, model::SignInSuccess, sign, ui};
use mhf_config::Config;
use mhf_mod_api::game::{GlobalData32, LaunchParams32};

pub(crate) fn run(
    configuration: Config<'_>,
    params: &mut LaunchParams32,
    global_data: &mut GlobalData32,
) -> Result<bool, String> {
    let section = configuration
        .read("sign")
        .map_err(|error| error.to_string())?;
    let settings = config::load(&section)?;
    let client = sign::Client::new(&settings.endpoint, settings.encoding)
        .map_err(|error| error.to_string())?;
    let credentials = CredentialStore::new(&client.credential_target());
    let Some(request) = ui::run(client, credentials, settings.encoding)? else {
        return Ok(false);
    };
    apply_sign_in(params, &request, settings.encoding)?;
    apply_global_sign_in(global_data, &request.sign_in)?;
    Ok(true)
}

fn apply_sign_in(
    params: &mut LaunchParams32,
    request: &ui::LaunchRequest,
    encoding: SignEncoding,
) -> Result<(), String> {
    let sign_in = &request.sign_in;
    let character = sign_in.selected_character(request.selected_character_id)?;
    let entrance_server = sign_in
        .entrance_servers
        .first()
        .expect("validated sign-in result has an entrance server");
    let entrance_host = entrance_server.ip().to_string();
    let alternate_address = format!("{}:8080", entrance_server.ip());

    let name = &mut params.selected_character_name;
    name.fill(0);
    // This legacy launch field is a byte prefix; keep its final NUL.
    let length = character.name.len().min(name.len() - 1);
    name[..length].copy_from_slice(&character.name[..length]);
    copy_c_string(
        "username",
        &mut params.username[..],
        &encoding.encode(&request.credentials.username)?,
    )?;
    copy_c_string(
        "password",
        &mut params.password[..],
        &encoding.encode(&request.credentials.password)?,
    )?;
    copy_c_string(
        "entrance server host",
        &mut params.entrance_server_host,
        entrance_host.as_bytes(),
    )?;
    copy_c_string(
        "entrance server address",
        &mut params.entrance_server_address,
        entrance_server.to_string().as_bytes(),
    )?;
    copy_c_string(
        "alternate entrance server address",
        &mut params.alternate_entrance_server_address,
        alternate_address.as_bytes(),
    )?;

    let character_id = u32::from(character.id);
    params.selected_character_id_1 = character_id;
    params.selected_character_id_2 = character_id;
    params.sign_session_id = u32::from(sign_in.session.session_id);
    params
        .sign_session_token
        .copy_from_slice(&sign_in.session.token);
    params.sign_session_issued_at =
        unix_timestamp32("session issued_at", &sign_in.session.issued_at)?;
    params.fixed_18ec_zero = 0;
    params.patch_server_count = 0;
    params.entrance_server_count = u32::try_from(sign_in.entrance_servers.len())
        .map_err(|_| "entrance server count exceeds u32".to_owned())?;
    params.selected_character_status = if character.is_new { 2 } else { 0 };
    params.course_rights = sign_in.rights.bits();
    params.selected_character_hr = u32::from(character.hr);
    params.character_ids.fill(0);
    for (target, character) in params.character_ids.iter_mut().zip(&sign_in.characters) {
        *target = u32::from(character.id);
    }
    params.fixed_1d58_one = 1;
    params.selected_character_gr = u32::from(character.gr);
    params.return_expires_at = unix_timestamp32("return_expires_at", &sign_in.return_expires_at)?;
    params.fixed_200c_one = 1;
    Ok(())
}

fn unix_timestamp32(field: &str, timestamp: &jiff::Timestamp) -> Result<u32, String> {
    u32::try_from(timestamp.as_second())
        .map_err(|_| format!("{field} is outside the unsigned 32-bit Unix timestamp range"))
}

fn apply_global_sign_in(data: &mut GlobalData32, sign_in: &SignInSuccess) -> Result<(), String> {
    let notice_slots = data.notices.len();
    let notice_bytes = data.notices[0].len();
    if sign_in.notices.len() > notice_slots {
        return Err(format!(
            "sign-in result has {} notices; at most {notice_slots} are supported",
            sign_in.notices.len()
        ));
    }
    let notices = sign_in
        .notices
        .iter()
        .enumerate()
        .map(|(index, notice)| {
            if notice.len() > notice_bytes {
                return Err(format!(
                    "sign-in notice {} is {} bytes; at most {notice_bytes} bytes are supported",
                    index + 1,
                    notice.len()
                ));
            }
            Ok(notice.as_slice())
        })
        .collect::<Result<Vec<_>, String>>()?;

    let festa_stall_slots = data.festa_stalls.len();
    let festa = sign_in
        .festa
        .as_ref()
        .map(|festa| {
            if festa.id == 0 {
                return Err("active Mezeporta Festa ID must not be 0".to_owned());
            }
            if festa.stalls.len() > festa_stall_slots {
                return Err(format!(
                    "Mezeporta Festa has {} stalls; at most {festa_stall_slots} are supported",
                    festa.stalls.len()
                ));
            }
            Ok((
                festa,
                unix_timestamp32("Festa starts_at", &festa.period.starts_at())?,
                unix_timestamp32("Festa expires_at", &festa.period.expires_at())?,
            ))
        })
        .transpose()?;

    data.notice_lengths.fill(0);
    data.notice_flags.fill(0);
    for notice in &mut data.notices {
        notice.fill(0);
    }
    for (index, notice) in notices.iter().enumerate() {
        data.notice_lengths[index] = notice.len() as u32;
        data.notices[index][..notice.len()].copy_from_slice(notice);
    }

    data.festa_id = 0;
    data.festa_starts_at = 0;
    data.festa_expires_at = 0;
    data.festa_solo_tickets = 0;
    data.festa_group_tickets = 0;
    data.festa_stalls.fill(0);
    if let Some((festa, starts_at, expires_at)) = festa {
        data.festa_id = festa.id;
        data.festa_starts_at = starts_at;
        data.festa_expires_at = expires_at;
        data.festa_solo_tickets = festa.solo_ticket_allowance;
        data.festa_group_tickets = festa.group_ticket_allowance;
        for (target, stall) in data.festa_stalls.iter_mut().zip(&festa.stalls) {
            *target = u32::from(*stall as u8);
        }
    }

    Ok(())
}

fn copy_c_string(field: &str, destination: &mut [u8], value: &[u8]) -> Result<(), String> {
    if value.len() >= destination.len() {
        return Err(format!(
            "{field} is {} bytes; at most {} bytes are supported",
            value.len(),
            destination.len().saturating_sub(1)
        ));
    }
    destination.fill(0);
    destination[..value.len()].copy_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SignEncoding;
    use crate::model::{IssuedSignSession, PasswordCredentials, SignCharacter, SignInSuccess};
    use jiff::{SignedDuration, Timestamp};
    use shrimpman_domain::{
        TimeRange,
        account::CourseRights,
        character::{CharacterId, Gender, WeaponType},
        mezeporta::{MezeportaFesta, MezeportaStall},
        session::SignSessionId,
    };

    #[test]
    fn sign_in_domain_maps_to_the_mhfo_abi() {
        let issued_at = Timestamp::new(1_700_000_000, 0).expect("valid test timestamp");
        let mut encoding = SignEncoding::Utf8;
        let mut request = ui::LaunchRequest {
            credentials: PasswordCredentials {
                username: "账号é".to_owned(),
                password: "密碼🙂".to_owned(),
            },
            sign_in: SignInSuccess {
                session: IssuedSignSession {
                    session_id: SignSessionId::from(1),
                    token: *b"KySJuNnR2PJu00Uw",
                    issued_at,
                },
                entrance_servers: vec!["127.0.0.1:53310".parse().expect("valid entrance server")],
                characters: vec![SignCharacter {
                    id: CharacterId::from(1),
                    name: "啊啊🙂".as_bytes().to_vec(),
                    gr: 50,
                    hr: 999,
                    weapon_type: WeaponType::GreatSword,
                    gender: Gender::Male,
                    last_sign_in_at: Some(issued_at),
                    is_new: false,
                }],
                notices: vec![b"Welcome".to_vec(), "テスト".as_bytes().to_vec()],
                last_character_id: Some(CharacterId::from(1)),
                rights: CourseRights::from_bits_retain(12),
                return_expires_at: Timestamp::new(i64::from(u32::MAX), 0)
                    .expect("valid expiry timestamp"),
                festa: Some(MezeportaFesta {
                    id: 7,
                    period: TimeRange::from_duration(issued_at, SignedDuration::from_hours(1)),
                    solo_ticket_allowance: 5,
                    group_ticket_allowance: 2,
                    stalls: vec![MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether],
                }),
            },
            selected_character_id: CharacterId::from(1),
        };
        let mut params = LaunchParams32::default();

        apply_sign_in(&mut params, &request, encoding).expect("domain config should fit the ABI");

        for (bytes, expected) in [
            (params.username.as_slice(), "账号é"),
            (params.password.as_slice(), "密碼🙂"),
            (params.selected_character_name.as_slice(), "啊啊🙂"),
        ] {
            assert_eq!(&bytes[..expected.len()], expected.as_bytes());
            assert_eq!(bytes[expected.len()], 0);
        }
        assert_eq!(params.selected_character_id_1, 1);
        assert_eq!(params.selected_character_id_2, 1);
        assert_eq!(params.sign_session_id, 1);
        assert_eq!(&params.sign_session_token, b"KySJuNnR2PJu00Uw");
        assert_eq!(params.sign_session_issued_at, 1_700_000_000);
        assert_eq!(params.entrance_server_count, 1);
        assert_eq!(&params.entrance_server_address[..15], b"127.0.0.1:53310");
        assert_eq!(&params.entrance_server_host[..9], b"127.0.0.1");
        assert_eq!(params.selected_character_hr, 999);
        assert_eq!(params.selected_character_gr, 50);
        assert_eq!(params.fixed_200c_one, 1);

        let mut global_data = GlobalData32::default();
        apply_global_sign_in(&mut global_data, &request.sign_in)
            .expect("global Sign data should fit the ABI");
        assert_eq!(global_data.notice_lengths[..2], [7, 9]);
        assert_eq!(&global_data.notices[0][..7], b"Welcome");
        assert_eq!(&global_data.notices[1][..9], "テスト".as_bytes());
        assert_eq!(global_data.festa_id, 7);
        assert_eq!(global_data.festa_starts_at, 1_700_000_000);
        assert_eq!(global_data.festa_expires_at, 1_700_003_600);
        assert_eq!(global_data.festa_solo_tickets, 5);
        assert_eq!(global_data.festa_group_tickets, 2);
        assert_eq!(global_data.festa_stalls[..2], [3, 4]);

        for (name, expected) in [
            ("角色名字测试".as_bytes(), "角色名字测".as_bytes()),
            (
                b"1234567890123456".as_slice(),
                b"123456789012345".as_slice(),
            ),
            (
                "12345678901234啊".as_bytes(),
                b"12345678901234\xE5".as_slice(),
            ),
            (
                b"\x83\x6e\x83\x93\xff\0tail".as_slice(),
                b"\x83\x6e\x83\x93\xff\0tail".as_slice(),
            ),
            ("短".as_bytes(), "短".as_bytes()),
            (b"".as_slice(), b"".as_slice()),
        ] {
            request.sign_in.characters[0].name = name.to_vec();
            apply_sign_in(&mut params, &request, encoding)
                .expect("character name should be truncated");
            assert_eq!(&params.selected_character_name[..expected.len()], expected);
            assert!(
                params.selected_character_name[expected.len()..]
                    .iter()
                    .all(|&byte| byte == 0)
            );
            assert_eq!(request.sign_in.characters[0].name, name);
            assert_eq!(params.character_ids[0], 1);
        }

        encoding = SignEncoding::ShiftJis;
        request.credentials.username = "ハンター".to_owned();
        request.credentials.password = "パスワード".to_owned();
        request.sign_in.session.token = [0xff; 16];
        request.sign_in.session.token[0] = 0;
        apply_sign_in(&mut params, &request, encoding)
            .expect("copy native credentials and opaque token");
        assert_eq!(&params.username[..9], b"\x83\x6e\x83\x93\x83\x5e\x81\x5b\0");
        assert_eq!(
            &params.password[..11],
            b"\x83\x70\x83\x58\x83\x8f\x81\x5b\x83\x68\0"
        );
        assert_eq!(params.sign_session_token, request.sign_in.session.token);
        request.sign_in.notices = vec![b"\xff\0\x83\x6e\0".to_vec()];
        apply_global_sign_in(&mut global_data, &request.sign_in).expect("copy opaque notice");
        assert_eq!(global_data.notice_lengths[0], 5);
        assert_eq!(&global_data.notices[0][..5], b"\xff\0\x83\x6e\0");
    }
}
