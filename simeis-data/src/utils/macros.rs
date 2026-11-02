#[macro_export]
macro_rules! define_type_enum {
    ($name:ident { $($y:ident),+ $(,)? }) => {
        #[derive(
            strum::EnumIter,
            strum::EnumString,
            strum::IntoStaticStr,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
        )]

        #[strum(serialize_all = "snake_case")]
        pub enum $name {
            #[strum(ascii_case_insensitive)]
            $(
                $y
            ),+
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                let name: &'static str = self.into();
                let conv = heck::AsSnakeCase(name).to_string();
                write!(f, "{}", conv)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where S: serde::Serializer {
                let name: &'static str = self.into();
                let conv = heck::AsSnakeCase(name).to_string();
                serializer.serialize_str(conv.as_str())
            }
        }

        impl<'de> serde::de::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let buf = <&str>::deserialize(deserializer)?;
                let conv = heck::AsSnakeCase(buf).to_string();
                <$name as std::str::FromStr>::from_str(&conv).map_err(serde::de::Error::custom)
            }
        }
    };
}
