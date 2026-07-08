use pl_protocol::UserQuestion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInputProjection {
    header: String,
    questions: Vec<UserInputQuestionProjection>,
}

impl UserInputProjection {
    pub fn header(&self) -> &str {
        &self.header
    }

    pub fn questions(&self) -> &[UserInputQuestionProjection] {
        &self.questions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInputQuestionProjection {
    id: String,
    question: String,
    options: Vec<UserInputOptionProjection>,
}

impl UserInputQuestionProjection {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn options(&self) -> &[UserInputOptionProjection] {
        &self.options
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInputOptionProjection {
    label: String,
    description: String,
}

impl UserInputOptionProjection {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

/// 将 `request_user_input` 的协议问题投影为宿主 UI 可消费的稳定形状。
///
/// pl-core 统一处理 header 选择、空 header 默认值和缺失 options 的归一化；
/// 宿主只负责把该投影映射到自身 Web/API 事件类型。
pub fn project_user_input_questions(
    questions: impl IntoIterator<Item = UserQuestion>,
) -> UserInputProjection {
    let mut header = None;
    let mut projected = Vec::new();
    for question in questions {
        if header.is_none() {
            let value = question.header.trim();
            if !value.is_empty() {
                header = Some(value.to_string());
            }
        }
        projected.push(UserInputQuestionProjection {
            id: question.id,
            question: question.question,
            options: question
                .options
                .unwrap_or_default()
                .into_iter()
                .map(|option| UserInputOptionProjection {
                    label: option.label,
                    description: option.description,
                })
                .collect(),
        });
    }
    UserInputProjection {
        header: header.unwrap_or_else(|| "Input".to_string()),
        questions: projected,
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::UserQuestionOption;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn user_input_projection_uses_first_non_empty_header() {
        let projection = project_user_input_questions(vec![
            UserQuestion {
                id: "first".to_string(),
                header: "  ".to_string(),
                question: "First?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            },
            UserQuestion {
                id: "second".to_string(),
                header: "  Scope  ".to_string(),
                question: "Second?".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![UserQuestionOption {
                    label: "Full".to_string(),
                    description: "Use full scope.".to_string(),
                }]),
            },
        ]);

        assert_eq!(projection.header(), "Scope");
        assert_eq!(projection.questions().len(), 2);
        assert_eq!(projection.questions()[0].id(), "first");
        assert_eq!(projection.questions()[0].question(), "First?");
        assert_eq!(projection.questions()[0].options(), &[]);
        assert_eq!(projection.questions()[1].id(), "second");
        assert_eq!(projection.questions()[1].question(), "Second?");
        assert_eq!(projection.questions()[1].options()[0].label(), "Full");
        assert_eq!(
            projection.questions()[1].options()[0].description(),
            "Use full scope."
        );
    }

    #[test]
    fn user_input_projection_defaults_blank_header_to_input() {
        let projection = project_user_input_questions(vec![UserQuestion {
            id: "scope".to_string(),
            header: String::new(),
            question: "Scope?".to_string(),
            is_other: false,
            is_secret: false,
            options: None,
        }]);

        assert_eq!(projection.header(), "Input");
    }
}
