#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_connector_abi::{
    decode_canonical, encode_canonical, pack_ptr_len, unpack_ptr_len, AbiVersion,
    CancellationGeneration, ConnectorEvent, ConnectorId, EventBody, EventSequence, LimitsProfileId,
    SessionId, WireError, MAX_EVENT_BYTES,
};

#[test]
fn packed_pointer_length_round_trips_unsigned_halves() {
    for (pointer, length) in [
        (0, 0),
        (1, 2),
        (u32::MAX, 0),
        (0, u32::MAX),
        (u32::MAX, u32::MAX),
    ] {
        assert_eq!(
            unpack_ptr_len(pack_ptr_len(pointer, length)),
            (pointer, length)
        );
    }
}

#[test]
fn abi_version_has_a_byte_frozen_canonical_vector() {
    let version = AbiVersion { major: 1, minor: 0 };
    let bytes = encode_canonical(&version).expect("version encodes");
    assert_eq!(bytes, [0xa2, 0x00, 0x01, 0x01, 0x00]);
    assert_eq!(decode_canonical::<AbiVersion>(&bytes), Ok(version));
}

#[test]
fn duplicate_unknown_and_non_shortest_fields_are_rejected() {
    let duplicate = [0xa3, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00];
    let unknown = [0xa3, 0x00, 0x01, 0x01, 0x00, 0x02, 0x00];
    let non_shortest = [0xa2, 0x18, 0x00, 0x01, 0x01, 0x00];
    let unordered = [0xa2, 0x01, 0x00, 0x00, 0x01];
    assert_eq!(
        decode_canonical::<AbiVersion>(&duplicate),
        Err(WireError::NonCanonical)
    );
    assert_eq!(
        decode_canonical::<AbiVersion>(&unknown),
        Err(WireError::NonCanonical)
    );
    assert_eq!(
        decode_canonical::<AbiVersion>(&non_shortest),
        Err(WireError::NonCanonical)
    );
    assert_eq!(
        decode_canonical::<AbiVersion>(&unordered),
        Err(WireError::NonCanonical)
    );
}

#[test]
fn floats_are_not_part_of_the_wire_vocabulary() {
    let float_major = [0xa2, 0x00, 0xf9, 0x3c, 0x00, 0x01, 0x00];
    assert!(matches!(
        decode_canonical::<AbiVersion>(&float_major),
        Err(WireError::Decode(_))
    ));
}

#[test]
fn notification_payload_bound_is_exact() {
    let event = |payload| ConnectorEvent {
        connector_id: ConnectorId::new("org.example.band").expect("valid connector id"),
        session_id: SessionId(7),
        sequence: EventSequence(9),
        cancellation_generation: CancellationGeneration(2),
        wall_time_ms: Some(1_700_000_000_000),
        body: EventBody::Notification {
            characteristic_id: "heart-rate".to_owned(),
            bytes: payload,
        },
    };
    assert!(encode_canonical(&event(vec![0; MAX_EVENT_BYTES])).is_ok());
    assert!(matches!(
        encode_canonical(&event(vec![0; MAX_EVENT_BYTES + 1])),
        Err(WireError::Bounds("event notification bytes"))
    ));
}

#[test]
fn ids_reject_empty_segments_unicode_and_edge_hyphens() {
    for invalid in ["example", "org..band", "org.Example.band", "org.-band"] {
        assert!(ConnectorId::new(invalid).is_err(), "accepted {invalid}");
    }
    assert!(LimitsProfileId::new("-mobile").is_err());
    assert!(LimitsProfileId::new("möbile").is_err());
}
