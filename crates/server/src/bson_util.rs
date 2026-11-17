use mongodb::bson::{spec::BinarySubtype, Binary, Bson};
use uuid::Uuid;

/// MongoDB's typed collection serializer stores `uuid::Uuid` values as generic
/// BSON binary data. `bson::to_bson(&Uuid)` produces a string instead, so queries
/// must use this helper to match the stored representation.
pub fn uuid_bson(id: Uuid) -> Bson {
    Bson::Binary(Binary {
        subtype: BinarySubtype::Generic,
        bytes: id.as_bytes().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_queried_as_generic_binary() {
        let id = Uuid::new_v4();
        let value = uuid_bson(id);
        let Bson::Binary(binary) = value else {
            panic!("expected binary BSON value");
        };
        assert_eq!(binary.subtype, BinarySubtype::Generic);
        assert_eq!(binary.bytes, id.as_bytes());
    }
}
