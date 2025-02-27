use crate::timestamp::{Timestamp, TimestampStyle};

use super::{MentionIter, MentionType, ParseMentionError, ParseMentionErrorType};
use std::{num::NonZeroU64, str::Chars};
use twilight_model::id::{
    marker::{ChannelMarker, EmojiMarker, RoleMarker, UserMarker},
    Id,
};

/// Parse mentions out of buffers.
///
/// While the syntax of mentions will be validated and the IDs within them
/// parsed, they won't be validated as being proper snowflakes or as real IDs in
/// use.
///
/// **Note** that this trait is sealed and is not meant to be manually
/// implemented.
pub trait ParseMention: private::Sealed {
    /// Leading sigil(s) of the mention after the leading arrow (`<`).
    ///
    /// In a channel mention, the sigil is `#`. In the case of a user mention,
    /// the sigil may be either `@` or `@!`.
    const SIGILS: &'static [&'static str];

    /// Parse a mention out of a buffer.
    ///
    /// This will not search the buffer for a mention and will instead treat the
    /// entire buffer as a mention.
    ///
    /// # Examples
    ///
    /// ```
    /// use twilight_mention::ParseMention;
    /// use twilight_model::id::{marker::{ChannelMarker, UserMarker}, Id};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// assert_eq!(
    ///     Id::<ChannelMarker>::new(123),
    ///     Id::parse("<#123>")?,
    /// );
    /// assert_eq!(
    ///     Id::<UserMarker>::new(456),
    ///     Id::parse("<@456>")?,
    /// );
    /// assert!(Id::<ChannelMarker>::parse("not a mention").is_err());
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`ParseMentionErrorType::LeadingArrow`] error type if the
    /// leading arrow is not present.
    ///
    /// Returns a [`ParseMentionErrorType::Sigil`] error type if the mention
    /// type's sigil is not present after the leading arrow.
    ///
    /// Returns a [`ParseMentionErrorType::TrailingArrow`] error type if the
    /// trailing arrow is not present after the ID.
    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized;

    /// Search a buffer for mentions and parse out any that are encountered.
    ///
    /// Unlike [`parse`], this will not error if anything that is indicative of
    /// a mention is encountered but did not successfully parse, such as a `<`
    /// but with no trailing mention sigil.
    ///
    /// [`parse`]: Self::parse
    #[must_use = "you must use the iterator to lazily parse mentions"]
    fn iter(buf: &str) -> MentionIter<'_, Self>
    where
        Self: Sized,
    {
        MentionIter::new(buf)
    }
}

impl ParseMention for Id<ChannelMarker> {
    const SIGILS: &'static [&'static str] = &["#"];

    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        parse_mention(buf, Self::SIGILS).map(|(id, _, _)| Id::from(id))
    }
}

impl ParseMention for Id<EmojiMarker> {
    const SIGILS: &'static [&'static str] = &[":"];

    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        parse_mention(buf, Self::SIGILS).map(|(id, _, _)| Id::from(id))
    }
}

impl ParseMention for MentionType {
    /// Sigils for any type of mention.
    ///
    /// Contains all of the sigils of every other type of mention.
    const SIGILS: &'static [&'static str] = &["#", ":", "@&", "@!", "@", "t:"];

    /// Parse a mention from a string slice.
    ///
    /// # Examples
    ///
    /// Returns [`ParseMentionErrorType::TimestampStyleInvalid`] if a timestamp
    /// style value is invalid.
    ///
    /// [`ParseMentionError::TimestampStyleInvalid`]: super::error::ParseMentionErrorType::TimestampStyleInvalid
    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        let (id, maybe_modifier, found) = parse_mention(buf, Self::SIGILS)?;

        for sigil in Id::<ChannelMarker>::SIGILS {
            if *sigil == found {
                return Ok(MentionType::Channel(Id::from(id)));
            }
        }

        for sigil in Id::<EmojiMarker>::SIGILS {
            if *sigil == found {
                return Ok(MentionType::Emoji(Id::from(id)));
            }
        }

        for sigil in Id::<RoleMarker>::SIGILS {
            if *sigil == found {
                return Ok(MentionType::Role(Id::from(id)));
            }
        }

        for sigil in Timestamp::SIGILS {
            if *sigil == found {
                let maybe_style = parse_maybe_style(maybe_modifier)?;

                return Ok(MentionType::Timestamp(Timestamp::new(
                    id.get(),
                    maybe_style,
                )));
            }
        }

        for sigil in Id::<UserMarker>::SIGILS {
            if *sigil == found {
                return Ok(MentionType::User(Id::from(id)));
            }
        }

        unreachable!("mention type must have been found");
    }
}

impl ParseMention for Id<RoleMarker> {
    const SIGILS: &'static [&'static str] = &["@&"];

    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        parse_mention(buf, Self::SIGILS).map(|(id, _, _)| Id::from(id))
    }
}

impl ParseMention for Timestamp {
    const SIGILS: &'static [&'static str] = &["t:"];

    /// Parse a timestamp from a string slice.
    ///
    /// # Examples
    ///
    /// Returns [`ParseMentionErrorType::TimestampStyleInvalid`] if the
    /// timestamp style value is invalid.
    ///
    /// [`ParseMentionError::TimestampStyleInvalid`]: super::error::ParseMentionErrorType::TimestampStyleInvalid
    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        let (unix, maybe_modifier, _) = parse_mention(buf, Self::SIGILS)?;

        Ok(Timestamp::new(
            unix.get(),
            parse_maybe_style(maybe_modifier)?,
        ))
    }
}

impl ParseMention for Id<UserMarker> {
    /// Sigils for User ID mentions.
    ///
    /// Unlike other IDs, user IDs have two possible sigils: `@!` and `@`.
    const SIGILS: &'static [&'static str] = &["@!", "@"];

    fn parse(buf: &str) -> Result<Self, ParseMentionError<'_>>
    where
        Self: Sized,
    {
        parse_mention(buf, Self::SIGILS).map(|(id, _, _)| Id::from(id))
    }
}

/// Parse a possible style value string slice into a [`TimestampStyle`].
///
/// # Errors
///
/// Returns [`ParseMentionErrorType::TimestampStyleInvalid`] if the timestamp
/// style value is invalid.
fn parse_maybe_style(value: Option<&str>) -> Result<Option<TimestampStyle>, ParseMentionError<'_>> {
    Ok(if let Some(modifier) = value {
        Some(
            TimestampStyle::try_from(modifier).map_err(|source| ParseMentionError {
                kind: ParseMentionErrorType::TimestampStyleInvalid { found: modifier },
                source: Some(Box::new(source)),
            })?,
        )
    } else {
        None
    })
}

/// # Errors
///
/// Returns [`ParseMentionErrorType::LeadingArrow`] if the leading arrow is not
/// present.
///
/// Returns [`ParseMentionErrorType::Sigil`] if the mention type's sigil is not
/// present after the leading arrow.
///
/// Returns [`ParseMentionErrorType::TrailingArrow`] if the trailing arrow is
/// not present after the ID.
fn parse_mention<'a>(
    buf: &'a str,
    sigils: &'a [&'a str],
) -> Result<(NonZeroU64, Option<&'a str>, &'a str), ParseMentionError<'a>> {
    let mut chars = buf.chars();

    let c = chars.next();

    if c.map_or(true, |c| c != '<') {
        return Err(ParseMentionError {
            kind: ParseMentionErrorType::LeadingArrow { found: c },
            source: None,
        });
    }

    let maybe_sigil = sigils.iter().find(|sigil| {
        if chars.as_str().starts_with(*sigil) {
            for _ in 0..sigil.chars().count() {
                chars.next();
            }

            return true;
        }

        false
    });

    let sigil = if let Some(sigil) = maybe_sigil {
        *sigil
    } else {
        return Err(ParseMentionError {
            kind: ParseMentionErrorType::Sigil {
                expected: sigils,
                found: chars.next(),
            },
            source: None,
        });
    };

    if sigil == ":" && !separator_sigil_present(&mut chars) {
        return Err(ParseMentionError {
            kind: ParseMentionErrorType::PartMissing {
                found: 1,
                expected: 2,
            },
            source: None,
        });
    }

    let end_position = chars
        .as_str()
        .find('>')
        .ok_or_else(|| ParseMentionError::trailing_arrow(None))?;
    let maybe_split_position = chars.as_str().find(':');

    let end_of_id_position = maybe_split_position.unwrap_or(end_position);

    let remaining = chars
        .as_str()
        .get(..end_of_id_position)
        .ok_or_else(|| ParseMentionError::trailing_arrow(None))?;

    let num = remaining.parse().map_err(|source| ParseMentionError {
        kind: ParseMentionErrorType::IdNotU64 { found: remaining },
        source: Some(Box::new(source)),
    })?;

    // If additional information - like a timestamp style - is present then we
    // can just get a subslice of the string via the split and ending positions.
    let style = maybe_split_position.and_then(|split_position| {
        chars.next();

        // We need to remove 1 so we don't catch the `>` in it.
        let style_end_position = end_position - 1;

        chars.as_str().get(split_position..style_end_position)
    });

    Ok((num, style, sigil))
}

// Don't use `Iterator::skip_while` so we can mutate `chars` in-place;
// `skip_while` is consuming.
fn separator_sigil_present(chars: &mut Chars<'_>) -> bool {
    for c in chars {
        if c == ':' {
            return true;
        }
    }

    false
}

/// Rust doesn't allow leaking private implementations, but if we make the trait
/// public in a private scope then it gets by the restriction and doesn't allow
/// Sealed to be named.
///
/// Yes, this is the correct way of sealing a trait:
///
/// <https://rust-lang.github.io/api-guidelines/future-proofing.html>
mod private {
    use super::super::MentionType;
    use crate::timestamp::Timestamp;
    use twilight_model::id::{
        marker::{ChannelMarker, EmojiMarker, RoleMarker, UserMarker},
        Id,
    };

    pub trait Sealed {}

    impl Sealed for Id<ChannelMarker> {}
    impl Sealed for Id<EmojiMarker> {}
    impl Sealed for MentionType {}
    impl Sealed for Id<RoleMarker> {}
    impl Sealed for Timestamp {}
    impl Sealed for Id<UserMarker> {}
}

#[cfg(test)]
mod tests {
    use super::{
        super::{MentionType, ParseMentionErrorType},
        private::Sealed,
        ParseMention,
    };
    use crate::{
        parse::ParseMentionError,
        timestamp::{Timestamp, TimestampStyle},
    };
    use static_assertions::assert_impl_all;
    use twilight_model::id::{
        marker::{ChannelMarker, EmojiMarker, RoleMarker, UserMarker},
        Id,
    };

    assert_impl_all!(Id<ChannelMarker>: ParseMention, Sealed);
    assert_impl_all!(Id<EmojiMarker>: ParseMention, Sealed);
    assert_impl_all!(MentionType: ParseMention, Sealed);
    assert_impl_all!(Id<RoleMarker>: ParseMention, Sealed);
    assert_impl_all!(Id<UserMarker>: ParseMention, Sealed);

    #[test]
    fn test_sigils() {
        assert_eq!(&["#"], Id::<ChannelMarker>::SIGILS);
        assert_eq!(&[":"], Id::<EmojiMarker>::SIGILS);
        assert_eq!(&["#", ":", "@&", "@!", "@", "t:"], MentionType::SIGILS);
        assert_eq!(&["@&"], Id::<RoleMarker>::SIGILS);
        assert_eq!(&["@!", "@"], Id::<UserMarker>::SIGILS);
    }

    #[test]
    fn test_parse_channel_id() {
        assert_eq!(Id::<ChannelMarker>::new(123), Id::parse("<#123>").unwrap());
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["#"],
                found: Some('@'),
            },
            Id::<ChannelMarker>::parse("<@123>").unwrap_err().kind(),
        );
    }

    #[test]
    fn test_parse_emoji_id() {
        assert_eq!(
            Id::<EmojiMarker>::new(123),
            Id::parse("<:name:123>").unwrap()
        );
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &[":"],
                found: Some('@'),
            },
            Id::<EmojiMarker>::parse("<@123>").unwrap_err().kind(),
        );
    }

    #[test]
    fn test_parse_mention_type() {
        assert_eq!(
            MentionType::Channel(Id::new(123)),
            MentionType::parse("<#123>").unwrap()
        );
        assert_eq!(
            MentionType::Emoji(Id::new(123)),
            MentionType::parse("<:name:123>").unwrap()
        );
        assert_eq!(
            MentionType::Role(Id::new(123)),
            MentionType::parse("<@&123>").unwrap()
        );
        assert_eq!(
            MentionType::User(Id::new(123)),
            MentionType::parse("<@123>").unwrap()
        );
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["#", ":", "@&", "@!", "@", "t:"],
                found: Some(';'),
            },
            MentionType::parse("<;123>").unwrap_err().kind(),
        );
    }

    #[test]
    fn test_parse_role_id() {
        assert_eq!(Id::<RoleMarker>::new(123), Id::parse("<@&123>").unwrap());
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["@&"],
                found: Some('@'),
            },
            Id::<RoleMarker>::parse("<@123>").unwrap_err().kind(),
        );
    }

    #[test]
    fn test_parse_timestamp() -> Result<(), ParseMentionError<'static>> {
        assert_eq!(Timestamp::new(123, None), Timestamp::parse("<t:123>")?);
        assert_eq!(
            Timestamp::new(123, Some(TimestampStyle::RelativeTime)),
            Timestamp::parse("<t:123:R>")?
        );
        assert_eq!(
            &ParseMentionErrorType::TimestampStyleInvalid { found: "?" },
            Timestamp::parse("<t:123:?>").unwrap_err().kind(),
        );

        Ok(())
    }

    #[test]
    fn test_parse_user_id() {
        assert_eq!(Id::<UserMarker>::new(123), Id::parse("<@123>").unwrap());
        assert_eq!(
            &ParseMentionErrorType::IdNotU64 { found: "&123" },
            Id::<UserMarker>::parse("<@&123>").unwrap_err().kind(),
        );
    }

    #[test]
    fn test_parse_id_wrong_sigil() {
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["@"],
                found: Some('#'),
            },
            super::parse_mention("<#123>", &["@"]).unwrap_err().kind(),
        );
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["#"],
                found: None,
            },
            super::parse_mention("<", &["#"]).unwrap_err().kind(),
        );
        assert_eq!(
            &ParseMentionErrorType::Sigil {
                expected: &["#"],
                found: None,
            },
            super::parse_mention("<", &["#"]).unwrap_err().kind(),
        );
    }
}
