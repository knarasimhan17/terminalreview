use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
/// A bare line number is ambiguous in a diff: deletions use the old file's
/// numbering while additions and context use the new file's.
pub(crate) enum Side {
    Old,
    New,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CommentState {
    Open,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Comment {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) side: Side,
    pub(crate) body: String,
    pub(crate) state: CommentState,
}

#[cfg(test)]
mod tests {
    use super::{Comment, CommentState, Side};

    #[test]
    fn comments_round_trip_through_json_without_losing_review_state() {
        let comment = Comment {
            path: "src/main.rs".to_owned(),
            line: 12,
            side: Side::New,
            body: "Handle the error at this boundary.".to_owned(),
            state: CommentState::Open,
        };

        let json =
            serde_json::to_string(&comment).expect("the comment model must serialize to JSON");
        let decoded: Comment =
            serde_json::from_str(&json).expect("serialized comments must deserialize");

        assert_eq!(
            decoded, comment,
            "JSON round trips must preserve every comment field"
        );
    }
}
