//! 枚举与稳定字符串标签之间的双向转换。
//!
//! 协议枚举需要持久化进数据库、序列化到 wire 格式、在投影层比对。
//! 同一套 enum↔label 映射此前在多个 crate 各写一份（`PureError` 版与 `anyhow` 版），
//! 产生字面重复。这里用统一的 trait 收敛映射定义，枚举只需 impl 一次。

use thiserror::Error;

/// 枚举值与稳定字符串标签之间的双向映射。
///
/// 实现者负责保证 `from_label(label(value)) == value`，且 label 字符串是
/// 数据库和 wire 格式可长期依赖的稳定标识。
pub trait LabeledEnum: Sized {
    /// 返回该枚举值的稳定字符串标签。
    fn label(&self) -> &'static str;

    /// 从稳定字符串标签恢复枚举值。
    ///
    /// # Errors
    ///
    /// 当标签不对应任何已知变体时返回 [`UnknownLabelError`]。
    fn from_label(label: &str) -> Result<Self, UnknownLabelError>;
}

/// 标签无法匹配任何已知枚举变体。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {enum_name} label {label:?}")]
pub struct UnknownLabelError {
    pub enum_name: &'static str,
    pub label: String,
}

impl UnknownLabelError {
    /// 构造一个带枚举名上下文的解析失败。
    pub fn new(enum_name: &'static str, label: impl Into<String>) -> Self {
        Self {
            enum_name,
            label: label.into(),
        }
    }
}

/// 逐变体展开生成 `LabeledEnum` 实现。
///
/// 每个 `(variant_path, label)` 对声明一个变体的稳定字符串。`variant_path` 必须是
/// 单元变体的完整路径（如 `Sample::Alpha`），不能是带数据变体或 `|` 分组模式。
/// 宏内部用穷尽 match，新增变体时编译期报错。
#[macro_export]
macro_rules! impl_labeled_enum {
    ($enum_ty:ty, $enum_name:literal, [$($variant_path:path => $label:literal),+ $(,)?]) => {
        impl $crate::labeled::LabeledEnum for $enum_ty {
            fn label(&self) -> &'static str {
                match self {
                    $($variant_path => $label,)+
                }
            }

            fn from_label(label: &str) -> Result<Self, $crate::labeled::UnknownLabelError> {
                match label {
                    $($label => Ok($variant_path),)+
                    other => Err($crate::labeled::UnknownLabelError::new($enum_name, other)),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Sample {
        Alpha,
        Beta,
    }

    impl_labeled_enum!(Sample, "sample", [Sample::Alpha => "alpha", Sample::Beta => "beta"]);

    #[test]
    fn round_trip_preserves_variant() {
        for value in [Sample::Alpha, Sample::Beta] {
            assert_eq!(Sample::from_label(value.label()).unwrap(), value);
        }
    }

    #[test]
    fn unknown_label_reports_enum_name_and_value() {
        let error = Sample::from_label("gamma").unwrap_err();
        assert_eq!(error.enum_name, "sample");
        assert_eq!(error.label, "gamma");
        assert_eq!(error.to_string(), r#"unknown sample label "gamma""#);
    }
}
