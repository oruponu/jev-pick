use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use thiserror::Error;

const QUESTION_LIMIT: usize = 300;
const OPTION_LIMIT: usize = 120;
const CONTEXT_LIMIT: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptionId {
    A,
    B,
    C,
    D,
}

impl fmt::Display for OptionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        };
        formatter.write_str(text)
    }
}

impl FromStr for OptionId {
    type Err = ParseOptionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "A" => Ok(Self::A),
            "B" => Ok(Self::B),
            "C" => Ok(Self::C),
            "D" => Ok(Self::D),
            _ => Err(ParseOptionIdError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid option ID")]
pub struct ParseOptionIdError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDecisionInput {
    pub question: String,
    pub a: String,
    pub b: String,
    pub c: Option<String>,
    pub d: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionOption {
    id: OptionId,
    text: String,
}

impl DecisionOption {
    pub fn id(&self) -> OptionId {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    question: String,
    context: Option<String>,
    options: Vec<DecisionOption>,
}

impl DecisionRequest {
    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    pub fn options(&self) -> &[DecisionOption] {
        &self.options
    }
}

impl TryFrom<RawDecisionInput> for DecisionRequest {
    type Error = InputError;

    fn try_from(raw: RawDecisionInput) -> Result<Self, Self::Error> {
        let question = normalize(raw.question);
        let a = normalize(raw.a);
        let b = normalize(raw.b);
        let c = raw.c.map(normalize);
        let d = raw.d.map(normalize);
        let context = raw.context.map(normalize).filter(|value| !value.is_empty());

        validate_required(&question, InputField::Question, QUESTION_LIMIT)?;
        validate_required(&a, InputField::Option(OptionId::A), OPTION_LIMIT)?;
        validate_required(&b, InputField::Option(OptionId::B), OPTION_LIMIT)?;
        validate_optional(c.as_deref(), OptionId::C)?;
        validate_optional(d.as_deref(), OptionId::D)?;

        if d.is_some() && c.is_none() {
            return Err(InputError::MissingPrecedingOption {
                option: OptionId::D,
                required: OptionId::C,
            });
        }

        if let Some(value) = context.as_deref()
            && value.chars().count() > CONTEXT_LIMIT
        {
            return Err(InputError::TooLong {
                field: InputField::Context,
                limit: CONTEXT_LIMIT,
            });
        }

        let mut options = vec![
            DecisionOption {
                id: OptionId::A,
                text: a,
            },
            DecisionOption {
                id: OptionId::B,
                text: b,
            },
        ];
        if let Some(text) = c {
            options.push(DecisionOption {
                id: OptionId::C,
                text,
            });
        }
        if let Some(text) = d {
            options.push(DecisionOption {
                id: OptionId::D,
                text,
            });
        }

        for later in 1..options.len() {
            for earlier in 0..later {
                if options[earlier].text == options[later].text {
                    return Err(InputError::DuplicateOptions {
                        first: options[earlier].id,
                        second: options[later].id,
                    });
                }
            }
        }

        Ok(Self {
            question,
            context,
            options,
        })
    }
}

fn normalize(value: String) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

fn validate_required(value: &str, field: InputField, limit: usize) -> Result<(), InputError> {
    if value.is_empty() {
        return Err(InputError::Empty { field });
    }
    if value.chars().count() > limit {
        return Err(InputError::TooLong { field, limit });
    }
    Ok(())
}

fn validate_optional(value: Option<&str>, id: OptionId) -> Result<(), InputError> {
    let Some(value) = value else {
        return Ok(());
    };
    validate_required(value, InputField::Option(id), OPTION_LIMIT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    Question,
    Option(OptionId),
    Context,
}

impl fmt::Display for InputField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Question => formatter.write_str("question"),
            Self::Option(id) => write!(formatter, "option {id}"),
            Self::Context => formatter.write_str("context"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InputError {
    #[error("{field} must not be empty")]
    Empty { field: InputField },
    #[error("{field} must contain at most {limit} characters")]
    TooLong { field: InputField, limit: usize },
    #[error("option {option} requires option {required}")]
    MissingPrecedingOption {
        option: OptionId,
        required: OptionId,
    },
    #[error("options {first} and {second} must be different")]
    DuplicateOptions { first: OptionId, second: OptionId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionResult {
    pub selected_id: OptionId,
    pub probabilities: BTreeMap<OptionId, f64>,
    pub confidence: f64,
    pub model: String,
    pub usage: TokenUsage,
}
