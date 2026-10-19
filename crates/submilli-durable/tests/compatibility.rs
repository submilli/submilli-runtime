//! Stored identities retain their encoding and authentication bindings.
use interpreter::runtime::filesystem::{DirectoryReader, FileReader, FileWriter, ReaderOptions};
use std::sync::Arc;
use submilli_durable::{
    codec::{self, Sealer},
    unit_of_work::JournalEntry,
};
use submilli_shared::secret_store::SecretCipher;

#[test]
fn filesystem_operation_associated_data_bind_scope_and_codec() {
    let cipher = Arc::new(SecretCipher::from_key([7; 32]));
    let sealer = Sealer::new(Some(cipher.clone()));
    let effect = JournalEntry {
        run_id: "scope".into(),
        id: "effect".into(),
        operation: "write".into(),
        version: 2,
        request: b"request".to_vec(),
        state: None,
    };
    let aad = r#"["filesystem-operation","scope","effect","write",2,"request"]"#;
    assert_eq!(codec::journal_entry_owner(&effect, "request").unwrap(), aad);
    let old = legacy_seal(&cipher, aad, b"request");
    assert_eq!(codec::open(&sealer, aad, &old).unwrap(), b"request");
    assert!(
        codec::open(
            &sealer,
            &codec::journal_entry_owner(&effect, "outcome").unwrap(),
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
fn resource_values_roundtrip_without_backend_services() {
    let reader = FileReader::new(
        "run".into(),
        "reader".into(),
        ReaderOptions::Bytes { chunk_size: 3 },
    );
    let encoded = serde_json::to_vec(&reader).unwrap();
    let reader: FileReader = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(reader.run_id.as_ref().unwrap().as_str(), "run");
    assert_eq!(reader.id.as_str(), "reader");
    assert_eq!(reader.options, ReaderOptions::Bytes { chunk_size: 3 });

    let writer = FileWriter::new("run".into(), "writer".into());
    let encoded = serde_json::to_vec(&writer).unwrap();
    let writer: FileWriter = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(writer.id.as_str(), "writer");
    assert!(!writer.closed);

    let listing = DirectoryReader::new("run".into(), "listing".into());
    let encoded = serde_json::to_vec(&listing).unwrap();
    let listing: DirectoryReader = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(listing.id.as_str(), "listing");
}
