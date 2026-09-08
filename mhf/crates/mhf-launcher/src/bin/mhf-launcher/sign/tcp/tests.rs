use super::*;
use shrimpman_domain::{
    character::{Gender, WeaponType},
    mezeporta::MezeportaStall,
};
use std::{net::TcpListener, sync::mpsc, thread, time::Instant};
use tokio::io::AsyncReadExt;

fn credentials() -> PasswordCredentials {
    PasswordCredentials {
        username: "猎人🦐".into(),
        password: "密码🔑".into(),
    }
}

// Encode the documented server layout independently of the response decoder.
fn success_payload() -> Vec<u8> {
    fn string(bytes: &mut Vec<u8>, text: &str) {
        bytes.push((text.len() + 1) as u8);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    let mut bytes = vec![1, 1, 1, 1]; // success, patch, entrance, character counts
    bytes.extend_from_slice(&7_u32.to_be_bytes());
    bytes.extend_from_slice(b"0123456789abcdef");
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    string(&mut bytes, "patch.example");
    string(&mut bytes, "127.0.0.1:53002");
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&999_u16.to_be_bytes());
    bytes.extend_from_slice(&13_u16.to_be_bytes());
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 1]); // female, initialized, old GR, u16 GR
    let mut name = [0; 16];
    name[..10].copy_from_slice("猎人🦐".as_bytes());
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&500_u16.to_be_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&[255, 0, 1]); // extended friend count
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&43_u32.to_be_bytes());
    string(&mut bytes, "好友");
    bytes.push(0); // guild members
    bytes.push(1); // notices
    bytes.extend_from_slice(&[0, 0]); // flags
    let notice = "欢迎猎人🦐";
    bytes.extend_from_slice(&((notice.len() + 1) as u16).to_be_bytes());
    bytes.extend_from_slice(notice.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(b"\0\x18smc\0\0\0\0\0nam\0\0\0\0\0msg\0\0\0\0\0");
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1_800_086_400_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&5_u32.to_be_bytes());
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    bytes.extend_from_slice(&1_800_003_600_u32.to_be_bytes());
    bytes.push(2);
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    bytes.extend_from_slice(&4_u32.to_be_bytes());
    bytes.extend_from_slice(&[2, 2, 4]);
    bytes
}

// Mezeporta/Erupe v9.2.1, server/signserver/dsgn_resp.go:
// makeSignInResp writes the fixed nonzero festival ID even when MezFes is off.
fn erupe_9_2_payload(festival_enabled: bool) -> Vec<u8> {
    let mut bytes = vec![1, 0, 1, 1]; // success, no patches, one entrance and character
    bytes.extend_from_slice(&u32::MAX.to_be_bytes()); // legacy login token number
    bytes.extend_from_slice(b"0123456789abcdef");
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    bytes.push(16);
    bytes.extend_from_slice(b"127.0.0.1:53310\0");
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&999_u16.to_be_bytes());
    bytes.extend_from_slice(&13_u16.to_be_bytes());
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 1]); // male, initialized, old GR, u16 GR
    let mut name = [0; 16];
    name[..8].copy_from_slice(b"\x83\x6e\x83\x93\x83\x5e\x81\x5b"); // ハンター in Shift-JIS
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&500_u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 5]); // reserved, friends, guildmates, notices
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 1, 0]); // empty length-prefixed filter
    bytes.extend_from_slice(&[
        0xca, 0x10, 0x4e, 0x20, 0, 1, 0, 0, // key type, version, empty key, no entries
        0xca, 0x11, 0, 1, 0x4e, 0x20, 0, 1, 0, // host type, enabled, version, empty host
    ]);
    bytes.extend_from_slice(&1_800_086_400_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0x0a51_97df_u32.to_be_bytes());
    if festival_enabled {
        bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
        bytes.extend_from_slice(&1_800_003_600_u32.to_be_bytes());
        bytes.push(2);
        bytes.extend_from_slice(&20_u32.to_be_bytes());
        bytes.extend_from_slice(&10_u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 10, 3, 6, 9, 4, 8, 5, 7]);
    } else {
        bytes.extend_from_slice(&[0; 8]); // start/end only; no ticket or stall counts
    }
    bytes
}

#[test]
fn decodes_erupe_9_2_with_and_without_a_festival() {
    for festival_enabled in [false, true] {
        let bytes = erupe_9_2_payload(festival_enabled);
        let sign_in = response::sign_in(&bytes).unwrap();
        assert_eq!(u32::from(sign_in.session.session_id), u32::MAX);
        assert_eq!(
            sign_in.entrance_servers,
            ["127.0.0.1:53310".parse().unwrap()]
        );
        assert_eq!(
            sign_in.characters[0].name,
            b"\x83\x6e\x83\x93\x83\x5e\x81\x5b\0\0\0\0\0\0\0\0"
        );
        assert_eq!(sign_in.characters[0].gr, 500);
        assert_eq!(sign_in.festa.is_some(), festival_enabled);
        if let Some(festa) = sign_in.festa {
            assert_eq!(festa.id, 0x0a51_97df);
            assert_eq!(
                (festa.solo_ticket_allowance, festa.group_ticket_allowance),
                (20, 10)
            );
            assert_eq!(festa.stalls.len(), 8);
        }
        for length in 0..bytes.len() {
            assert!(
                response::sign_in(&bytes[..length]).is_err(),
                "accepted {length} of {} bytes, festival_enabled={festival_enabled}",
                bytes.len(),
            );
        }
    }
}

#[test]
fn rejects_partial_festival_details_after_zero_dates() {
    let mut bytes = erupe_9_2_payload(false);
    bytes.push(2); // details started: both ticket allowances and the stall count are required
    bytes.extend_from_slice(&20_u32.to_be_bytes());
    bytes.extend_from_slice(&10_u32.to_be_bytes());
    assert!(response::sign_in(&bytes).is_err());
}

#[test]
fn decodes_session_characters_notices_and_festival() {
    let sign_in = response::sign_in(&success_payload()).unwrap();
    assert_eq!(sign_in.session.session_id, SignSessionId::from(7));
    assert_eq!(sign_in.session.token, *b"0123456789abcdef");
    assert_eq!(sign_in.session.issued_at.as_second(), 1_800_000_000);
    assert_eq!(
        sign_in.entrance_servers,
        ["127.0.0.1:53002".parse().unwrap()]
    );
    assert_eq!(sign_in.last_character_id, Some(CharacterId::from(42)));
    assert_eq!(sign_in.rights.bits(), 12);
    assert_eq!(sign_in.return_expires_at.as_second(), 1_800_086_400);
    let character = &sign_in.characters[0];
    assert_eq!(character.name, "猎人🦐\0\0\0\0\0\0".as_bytes());
    assert_eq!((character.hr, character.gr), (999, 500));
    assert_eq!(character.gender, Gender::Female);
    assert_eq!(character.weapon_type, WeaponType::MagnetSpike);
    assert!(!character.is_new);
    assert_eq!(sign_in.notices, ["欢迎猎人🦐\0".as_bytes()]);
    let festa = sign_in.festa.unwrap();
    assert_eq!(festa.id, 5);
    assert_eq!(festa.period.expires_at().as_second(), 1_800_003_600);
    assert_eq!(
        (festa.solo_ticket_allowance, festa.group_ticket_allowance),
        (3, 4)
    );
    assert_eq!(
        festa.stalls,
        [
            MezeportaStall::TokotokoPartnya,
            MezeportaStall::VolpakkunTogether
        ]
    );
}

#[test]
fn decodes_a_first_login_with_no_optional_metadata() {
    let mut bytes = vec![1, 0, 0, 1];
    bytes.extend_from_slice(&7_u32.to_be_bytes());
    bytes.extend_from_slice(b"0123456789abcdef");
    bytes.extend_from_slice(&1_800_000_000_u32.to_be_bytes());
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&[0; 8]); // HR, weapon, last sign-in
    bytes.extend_from_slice(&[0, 1, 0, 1]); // male, pending, old GR, u16 GR
    bytes.extend_from_slice(&[0; 52]); // name, description, GR, reserved
    bytes.extend_from_slice(&[0; 3]); // friends, guild members, notices
    bytes.extend_from_slice(&0_u32.to_be_bytes()); // no last character
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(b"\0\x18smc\0\0\0\0\0nam\0\0\0\0\0msg\0\0\0\0\0");
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1_800_086_400_u32.to_be_bytes());
    bytes.extend_from_slice(&[0; 16]); // reserved, festival ID, starts, expires
    bytes.push(2);
    bytes.extend_from_slice(&[0; 9]); // two zero ticket allowances, no stalls
    let sign_in = response::sign_in(&bytes).unwrap();
    assert!(sign_in.entrance_servers.is_empty());
    assert!(sign_in.notices.is_empty());
    assert!(sign_in.festa.is_none());
    assert!(sign_in.last_character_id.is_none());
    assert_eq!(sign_in.characters[0].id, CharacterId::from(42));
    assert!(sign_in.characters[0].is_new);
    assert_eq!(sign_in.characters[0].name, [0; 16]);
    assert!(sign_in.characters[0].last_sign_in_at.is_none());
}

#[test]
fn rejects_truncated_and_invalid_sign_in_data() {
    let bytes = success_payload();
    for length in 0..bytes.len() {
        assert!(
            response::sign_in(&bytes[..length]).is_err(),
            "accepted {length} bytes"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(response::sign_in(&trailing).is_err());
    let mut too_many = bytes.clone();
    too_many[3] = 17;
    assert!(response::sign_in(&too_many).is_err());
    let mut no_session = bytes.clone();
    no_session[4..8].fill(0);
    assert!(response::sign_in(&no_session).is_err());
    let mut invalid_entrance = bytes;
    let address_offset = invalid_entrance
        .windows(15)
        .position(|bytes| bytes == b"127.0.0.1:53002")
        .unwrap();
    invalid_entrance[address_offset] = 0xff;
    assert!(response::sign_in(&invalid_entrance).is_err());
}

#[test]
fn preserves_opaque_names_notices_and_tokens() {
    let original = success_payload();
    let name_offset = original
        .windows(10)
        .position(|bytes| bytes == "猎人🦐".as_bytes())
        .unwrap();
    let notice_offset = original
        .windows(17)
        .position(|bytes| bytes == "欢迎猎人🦐\0".as_bytes())
        .unwrap();
    let token = *b"\0\x80\xff3456789abcdef";
    let mut terminated_notice = [0xff; 17];
    terminated_notice[2] = 0;
    terminated_notice[16] = 0;
    for (name, notice) in [
        (*b"1234567890abcdef", terminated_notice),
        ([0xff; 16], [0xfe; 17]),
    ] {
        let mut bytes = original.clone();
        bytes[8..24].copy_from_slice(&token);
        bytes[name_offset..name_offset + 16].copy_from_slice(&name);
        bytes[notice_offset..notice_offset + 17].copy_from_slice(&notice);
        let sign_in = response::sign_in(&bytes).unwrap();
        assert_eq!(sign_in.session.token, token);
        assert_eq!(sign_in.characters[0].name, name);
        assert_eq!(sign_in.notices, [notice]);
    }
}

#[test]
fn ignores_text_encoding_and_terminators_in_unused_fields() {
    let mut bytes = success_payload();
    for value in [b"patch.example\0".as_slice(), "好友\0".as_bytes()] {
        let offset = bytes
            .windows(value.len())
            .position(|bytes| bytes == value)
            .unwrap();
        bytes[offset..offset + value.len()].fill(0xff);
    }
    let name_offset = bytes
        .windows(10)
        .position(|bytes| bytes == "猎人🦐".as_bytes())
        .unwrap();
    bytes[name_offset + 16..name_offset + 48].fill(0xff);
    assert!(response::sign_in(&bytes).is_ok());

    let mut bytes = erupe_9_2_payload(false);
    let cap_link_offset = bytes
        .windows(4)
        .position(|bytes| bytes == [0xca, 0x10, 0x4e, 0x20])
        .unwrap();
    bytes[cap_link_offset + 6] = 0xff;
    bytes[cap_link_offset + 16] = 0xfe;
    assert!(response::sign_in(&bytes).is_ok());
}

#[test]
fn maps_protocol_errors_and_rejects_ambiguous_credentials() {
    assert_eq!(
        response::sign_in(&[3]).unwrap_err().code(),
        Some("illegal_input")
    );
    assert_eq!(
        response::sign_in(&[12]).unwrap_err().code(),
        Some("wrong_password")
    );
    assert!(matches!(
        response::sign_in(&[255]),
        Err(Error::TcpResponse(255))
    ));
    for (username, password) in [("", "p"), ("user+", "p"), ("u\0ser", "p"), ("user", "p\0x")] {
        let credentials = PasswordCredentials {
            username: username.into(),
            password: password.into(),
        };
        assert!(sign_in_request(&credentials, false, SignEncoding::Utf8).is_err());
        assert!(sign_in_request(&credentials, true, SignEncoding::Utf8).is_err());
    }
}

#[test]
fn accepts_hostnames_and_ipv6_but_rejects_invalid_endpoints() {
    for address in ["localhost:53000", "127.0.0.1:53000", "[::1]:53000"] {
        assert!(
            Client::new(address, SignEncoding::Utf8).is_ok(),
            "{address}"
        );
    }
    for address in [
        "",
        "localhost",
        "localhost:0",
        "localhost:65536",
        "tcp://localhost:53000",
        "user@localhost:53000",
        "localhost:53000/path",
    ] {
        assert!(
            Client::new(address, SignEncoding::Utf8).is_err(),
            "{address}"
        );
    }
}

fn accept(listener: &TcpListener) -> std::net::TcpStream {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(true).unwrap();
                return stream;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "request did not connect"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("failed to accept: {error}"),
        }
    }
}

#[test]
fn exchanges_encrypted_login_create_and_delete_requests() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for operation in ["sign_in", "create", "delete"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let client = Client::new(
            &listener.local_addr().unwrap().to_string(),
            SignEncoding::Utf8,
        )
        .unwrap();
        let (sender, receiver) = mpsc::channel();
        if operation == "delete" {
            client
                .delete_character(
                    SignSessionId::from(7),
                    *b"0123456789abcdef",
                    CharacterId::from(42),
                    move |result| {
                        sender
                            .send(result.map(|id| assert_eq!(id, CharacterId::from(42))))
                            .unwrap();
                    },
                )
                .unwrap();
        } else {
            client
                .sign_in(&credentials(), operation == "create", move |result| {
                    sender
                        .send(result.map(|sign_in| {
                            assert_eq!(sign_in.session.session_id, SignSessionId::from(7))
                        }))
                        .unwrap();
                })
                .unwrap();
        }
        let stream = accept(&listener);
        runtime.block_on(async {
            let mut stream = TcpStream::from_std(stream).unwrap();
            let mut initialization = [255; 8];
            stream.read_exact(&mut initialization).await.unwrap();
            assert_eq!(initialization, [0; 8]);
            let mut connection = MhfConnection::new(stream);
            let request = timeout(Duration::from_secs(3), connection.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let expected = match operation {
                "sign_in" => "DSGN:100\0猎人🦐\0密码🔑\0\0".as_bytes().to_vec(),
                "create" => "DSGN:100\0猎人🦐+\0密码🔑\0\0".as_bytes().to_vec(),
                _ => b"DELETE:100\x000123456789abcdef\0\0\0\0\x2a\0\0\0\x07".to_vec(),
            };
            assert_eq!(request, expected);
            connection
                .send(
                    if operation == "delete" {
                        vec![1]
                    } else {
                        success_payload()
                    }
                    .into(),
                )
                .await
                .unwrap();
        });
        receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
    }
}

#[test]
fn accepts_an_erupe_error_frame_after_a_dsgn_100_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = Client::new(
        &listener.local_addr().unwrap().to_string(),
        SignEncoding::ShiftJis,
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    client
        .sign_in(
            &PasswordCredentials {
                username: "hunter".into(),
                password: "password".into(),
            },
            false,
            move |result| sender.send(result).unwrap(),
        )
        .unwrap();
    let stream = accept(&listener);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut stream = TcpStream::from_std(stream).unwrap();
        let mut initialization = [255; 8];
        stream.read_exact(&mut initialization).await.unwrap();
        assert_eq!(initialization, [0; 8]);
        let (reader, mut writer) = stream.into_split();
        let mut connection = MhfConnection::new(reader);
        let request = timeout(Duration::from_secs(3), connection.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        // Erupe 9.2 closes commands other than DSGN:100/DLTSKEYSIGN:100.
        if request.as_ref() == b"DSGN:100\0hunter\0password\0\0" {
            // Erupe's encrypted SIGN_EABORT response, captured independently
            // of the Rust encoder. It must reach the status decoder, not EOF.
            writer
                .write_all(&[
                    0x03, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x00, 0x05, 0x00,
                    0x05, 0x17,
                ])
                .await
                .unwrap();
        }
    });
    let result = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(result, Err(Error::TcpResponse(5))), "{result:?}");
}

#[test]
fn handles_fragmented_encrypted_responses() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = Client::new(
        &listener.local_addr().unwrap().to_string(),
        SignEncoding::Utf8,
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    client
        .sign_in(&credentials(), false, move |result| {
            sender.send(result).unwrap()
        })
        .unwrap();
    let stream = accept(&listener);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (writer, mut reader) = tokio::io::duplex(4096);
        let mut encoder = MhfConnection::new(writer);
        encoder.send(success_payload().into()).await.unwrap();
        drop(encoder);
        let mut encrypted = Vec::new();
        reader.read_to_end(&mut encrypted).await.unwrap();
        let mut stream = TcpStream::from_std(stream).unwrap();
        for fragment in encrypted.chunks(3) {
            stream.write_all(fragment).await.unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        // Keep the peer alive until the callback proves that EOF is unnecessary.
        let sign_in = receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(sign_in.characters[0].name, "猎人🦐\0\0\0\0\0\0".as_bytes());
    });
}

#[test]
fn all_operations_time_out_on_missing_or_incomplete_responses() {
    thread::scope(|scope| {
        for operation in ["sign_in", "create", "delete"] {
            for partial in [false, true] {
                scope.spawn(move || {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    listener.set_nonblocking(true).unwrap();
                    let client = Client::new(
                        &listener.local_addr().unwrap().to_string(),
                        SignEncoding::Utf8,
                    )
                    .unwrap();
                    let (sender, receiver) = mpsc::channel();
                    let started = Instant::now();
                    if operation == "delete" {
                        client
                            .delete_character(
                                SignSessionId::from(7),
                                *b"0123456789abcdef",
                                CharacterId::from(42),
                                move |result| {
                                    sender.send(result.map(|_| ())).unwrap();
                                },
                            )
                            .unwrap();
                    } else {
                        client
                            .sign_in(&credentials(), operation == "create", move |result| {
                                sender.send(result.map(|_| ())).unwrap();
                            })
                            .unwrap();
                    }
                    let mut stream = accept(&listener);
                    if partial {
                        stream.set_nonblocking(false).unwrap();
                        std::io::Write::write_all(&mut stream, &[3, 3, 0, 0, 0, 10, 0]).unwrap();
                    }
                    let result = receiver
                        .recv_timeout(REQUEST_TIMEOUT + Duration::from_secs(3))
                        .unwrap();
                    assert!(
                        matches!(result, Err(Error::Timeout)),
                        "{operation}: {result:?}"
                    );
                    assert!(started.elapsed() < REQUEST_TIMEOUT + Duration::from_secs(3));
                });
            }
        }
    });
}
