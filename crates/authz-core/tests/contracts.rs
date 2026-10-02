use authz_core::*;
use serde_json::{Value, json};
#[test]
fn m0_typed_fixtures_and_boundaries() {
    let v: Value =
        serde_json::from_str(include_str!("../../../spec/examples/contracts.json")).unwrap();
    for name in ["write", "revoke"] {
        let w: WriteRequest =
            strict_json(&serde_json::to_vec(&v[name]["request"]).unwrap()).unwrap();
        w.validate().unwrap();
    }
    let c: CheckRequest =
        strict_json(&serde_json::to_vec(&v["check"]["request"]).unwrap()).unwrap();
    c.batch().validate().unwrap();
    for s in ["", ".x", "a/b", "é", &"a".repeat(129)] {
        assert!(Id::new(s).is_err());
    }
    assert!(Id::new("a".repeat(128)).is_ok());
    for raw in ["0", "null", "\"01\"", "\"-1\"", "\"9223372036854775808\""] {
        assert!(strict_json::<Revision>(raw.as_bytes()).is_err());
    }
    assert_eq!(
        strict_json::<Revision>(b"\"9223372036854775807\"")
            .unwrap()
            .0,
        i64::MAX
    );
    let mut b = v["batch_check"]["request"].clone();
    for n in [0, 100, 101] {
        b["checks"] = Value::Array(vec![b["checks"][0].clone(); n]);
        if n == 0 {
            b["checks"] = json!([])
        };
        let mut batch: BatchRequest =
            serde_json::from_value(v["batch_check"]["request"].clone()).unwrap();
        batch.checks = vec![batch.checks[0].clone(); n];
        assert_eq!(batch.validate().is_ok(), n == 100);
    }
    let mut c = v["check"]["request"].clone();
    c["extra"] = json!(true);
    assert!(strict_json::<CheckRequest>(&serde_json::to_vec(&c).unwrap()).is_err());
    for raw in [
        br#"{"kind":"user","kind":"user","id":"a"}"#.as_slice(),
        br#"{"kind":"user","id":null}"#.as_slice(),
    ] {
        assert!(strict_json::<Entity>(raw).is_err());
    }
    assert!(
        strict_json::<Value>(format!("{}0{}", "[".repeat(16), "]".repeat(16)).as_bytes()).is_ok()
    );
    assert!(
        strict_json::<Value>(format!("{}0{}", "[".repeat(17), "]".repeat(17)).as_bytes()).is_err()
    );
    assert_eq!(
        strict_json::<Value>(&vec![b' '; MAX_BODY + 1]).unwrap_err(),
        Fault::PayloadTooLarge
    );
}
#[test]
fn m0_canonicalization_golden() {
    let a=br#"{"operations":[{"op":"put_entity","entity":{"kind":"user","id":"u"}}],"expected_revision":"0"}"#;
    let w: WriteRequest = strict_json(a).unwrap();
    assert_eq!(
        String::from_utf8(w.canonical().unwrap()).unwrap(),
        r#"{"expected_revision":"0","operations":[{"entity":{"id":"u","kind":"user"},"op":"put_entity"}]}"#
    );
    let b=br#"{ "expected_revision":"0", "operations":[{"entity":{"id":"\u0075","kind":"user"},"op":"put_entity"}] }"#;
    let other: WriteRequest = strict_json(b).unwrap();
    assert_eq!(w.hash().unwrap(), other.hash().unwrap());
    assert_eq!(
        hex::encode(w.hash().unwrap()),
        "815e5dc52e932e6f49b5074cc250816d558d5a1204edd1eed54213f720596ce8"
    );
    let mut different = other.clone();
    different.expected_revision = Revision(1);
    assert_ne!(w.hash().unwrap(), different.hash().unwrap());
    let mut duplicate = w.clone();
    duplicate.operations.push(duplicate.operations[0].clone());
    assert_eq!(duplicate.validate(), Err(Fault::DuplicateOperation));
    let mut multi = w.clone();
    multi.operations.push(Operation::PutEntity {
        entity: Entity {
            kind: Kind::Team,
            id: Id::new("dev").unwrap(),
        },
    });
    let mut reversed = multi.clone();
    reversed.operations.reverse();
    assert_eq!(multi.hash().unwrap(), reversed.hash().unwrap());
}
