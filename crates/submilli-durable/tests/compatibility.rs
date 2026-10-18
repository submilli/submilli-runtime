//! Stored identities retain their encoding and authentication bindings.
use interpreter::runtime::filesystem::ResourceDescriptor;
use std::sync::Arc;
use submilli_durable::{
    codec::{self, Sealer},
    unit_of_work::Operation,
};
use submilli_shared::secret_store::SecretCipher;

#[test]
fn filesystem_operation_associated_data_bind_scope_and_codec() {
    let cipher = Arc::new(SecretCipher::from_key([7; 32]));
    let sealer = Sealer::new(Some(cipher.clone()));
    let effect = Operation {
        run_id: "scope".into(),
        id: "effect".into(),
        operation: "write".into(),
        version: 2,
        request: b"request".to_vec(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    };
    let aad = r#"["filesystem-operation","scope","effect","write",2,"request"]"#;
    assert_eq!(codec::operation_owner(&effect, "request").unwrap(), aad);
    let old = legacy_seal(&cipher, aad, b"request");
    assert_eq!(codec::open(&sealer, aad, &old).unwrap(), b"request");
    assert!(
        codec::open(
            &sealer,
            &codec::operation_owner(&effect, "outcome").unwrap(),
            &old
        )
        .is_err()
    );
}
fn legacy_seal(cipher: &SecretCipher, owner: &str, bytes: &[u8]) -> Vec<u8> {
    let mut result = vec![2];
    result.extend_from_slice(&cipher.key_id());
    result.extend_from_slice(&cipher.seal(bytes, owner.as_bytes()).unwrap());
    result
}

#[test]
fn typed_resource_ids_preserve_descriptor_encoding() {
    let stored = r#"{"scope":"run","id":"opening-effect","kind":"Writer"}"#;
    let descriptor: ResourceDescriptor = serde_json::from_str(stored).unwrap();
    assert_eq!(descriptor.run_id.as_str(), "run");
    assert_eq!(descriptor.id.as_str(), "opening-effect");
    assert_eq!(serde_json::to_string(&descriptor).unwrap(), stored);
}
