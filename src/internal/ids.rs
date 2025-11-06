use std::fmt;
use std::num::NonZeroU64;

macro_rules! define_id_type {
    ($name:ident, $doc:literal) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        #[doc = $doc]
        pub(crate) struct $name(NonZeroU64);

        impl From<NonZeroU64> for $name {
            fn from(value: NonZeroU64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for NonZeroU64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0.get()
            }
        }

        impl TryFrom<u64> for $name {
            type Error = &'static str;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or("ID values must be non-zero")
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

define_id_type!(
    ObservableId,
    "Unique identifier for observable entities registered in the runtime."
);
define_id_type!(
    DerivationId,
    "Unique identifier for derivations (reactions or computed values)."
);
define_id_type!(
    ReactionId,
    "Unique identifier for reaction handlers scheduled by the runtime."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_round_trip() {
        let raw = NonZeroU64::new(42).unwrap();
        let id = ObservableId::from(raw);
        assert_eq!(NonZeroU64::from(id), raw);
        assert_eq!(u64::from(id), 42);
    }

    #[test]
    fn test_id_rejects_zero() {
        assert!(ObservableId::try_from(0).is_err());
        assert!(DerivationId::try_from(0).is_err());
        assert!(ReactionId::try_from(0).is_err());
    }
}
