//! Fixed object and filesystem values from before the crate extraction. These fixtures
//! deliberately avoid using the new encoder to construct their expected input.
use std::sync::Arc;
use submilli_durable::{codec, object::ObjectId, recovery::Intent, sealing::Sealer, state::Effect};
use submilli_shared::secret_store::SecretCipher;

#[test]
fn version_one_filesystem_intent_keeps_its_exact_encoding() {
    let bytes = br#"{"version":1,"workspace":{"id":"00000000-0000-4000-8000-000000000001","volumes":[{"mount":"/","root":{"dev":1,"ino":2}}]},"effect":"00000000-0000-4000-8000-000000000002","mutation":{"CreateDirectory":{"path":"/created"}},"source":{"mount":"/","parent":".","parent_identity":{"dev":1,"ino":2},"name":"created"},"destination":null,"original":null,"destination_original":null,"candidate":null,"original_bytes":0,"original_sha256":null,"original_directory":false}"#;
    let intent = Intent::decode(bytes).unwrap();
    assert_eq!(intent.encode().unwrap(), bytes);
}

#[test]
fn shared_effect_and_object_associated_data_keep_the_original_format() {
    let cipher = Arc::new(SecretCipher::from_key([7; 32]));
    let sealer = Sealer::new(Some(cipher.clone()));
    let effect = Effect {
        run: "run".into(),
        id: "effect".into(),
        operation: "filesystem.mutation".into(),
        version: 1,
        request: b"request".to_vec(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    };
    let aad = r#"["durable-effect","run","effect","filesystem.mutation",1,"request"]"#;
    assert_eq!(codec::effect_owner(&effect, "request").unwrap(), aad);
    let old = legacy_seal(&cipher, aad, b"request");
    assert_eq!(codec::open(&sealer, aad, &old).unwrap(), b"request");
    assert!(
        codec::open(
            &sealer,
            &codec::effect_owner(&effect, "outcome").unwrap(),
            &old
        )
        .is_err()
    );
    let id = ObjectId::parse("00000000-0000-4000-8000-000000000003".into()).unwrap();
    let content = legacy_seal(&cipher, "durable-object:00000000-0000-4000-8000-000000000003:content", br#"{"bytes":4,"sha256":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#);
    let retention = legacy_seal(
        &cipher,
        "durable-object:00000000-0000-4000-8000-000000000003:retention",
        br#"[{"Run":"run"},{"Workspace":"workspace"}]"#,
    );
    let (content, retention) =
        submilli_durable::object_codec::decode(&id, Some(&content), &retention, &sealer).unwrap();
    assert_eq!(content.unwrap().bytes, 4);
    assert_eq!(retention.len(), 2);
}
fn legacy_seal(cipher: &SecretCipher, owner: &str, bytes: &[u8]) -> Vec<u8> {
    let mut result = vec![2];
    result.extend_from_slice(&cipher.key_id());
    result.extend_from_slice(&cipher.seal(bytes, owner.as_bytes()).unwrap());
    result
}
