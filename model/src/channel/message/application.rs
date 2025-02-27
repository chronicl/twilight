use crate::{
    id::{marker::ApplicationMarker, Id},
    util::image_hash::ImageHash,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct MessageApplication {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_image: Option<ImageHash>,
    pub description: String,
    pub icon: Option<ImageHash>,
    pub id: Id<ApplicationMarker>,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::MessageApplication;
    use crate::{id::Id, test::image_hash};
    use serde_test::Token;

    #[test]
    fn test_message_application() {
        let value = MessageApplication {
            cover_image: Some(image_hash::COVER),
            description: "a description".to_owned(),
            icon: Some(image_hash::ICON),
            id: Id::new(1),
            name: "application".to_owned(),
        };

        serde_test::assert_tokens(
            &value,
            &[
                Token::Struct {
                    name: "MessageApplication",
                    len: 5,
                },
                Token::Str("cover_image"),
                Token::Some,
                Token::Str(image_hash::COVER_INPUT),
                Token::Str("description"),
                Token::Str("a description"),
                Token::Str("icon"),
                Token::Some,
                Token::Str(image_hash::ICON_INPUT),
                Token::Str("id"),
                Token::NewtypeStruct { name: "Id" },
                Token::Str("1"),
                Token::Str("name"),
                Token::Str("application"),
                Token::StructEnd,
            ],
        );
    }
}
