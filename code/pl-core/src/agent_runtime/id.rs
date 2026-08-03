use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

macro_rules! runtime_id {
    ($name:ident, $label:literal, $prefix:literal) => {
        #[doc = concat!($label, " 的非空透明字符串标识。")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// 校验并创建标识。
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(PureError::ConfigError(
                        concat!($label, " cannot be empty").into(),
                    ));
                }
                Ok(Self(value))
            }

            /// 生成进程内唯一、便于诊断的标识。
            pub fn generate() -> Self {
                static SEQUENCE: AtomicU64 = AtomicU64::new(0);
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
                Self(format!("{}_{millis:x}_{sequence:x}", $prefix))
            }

            /// 返回标识文本。
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// 消费标识并返回字符串。
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = PureError;

            fn from_str(value: &str) -> Result<Self> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

runtime_id!(AgentId, "agent id", "agent");
runtime_id!(SessionId, "session id", "session");
runtime_id!(TurnId, "turn id", "turn");
