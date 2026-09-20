//! Pure, localized Discord display data.

use crate::decision::{DecisionRequest, DecisionResult, InputError, InputField};
use crate::language::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDecision {
    pub title: String,
    pub description: String,
    pub fields: Vec<RenderedField>,
    pub footer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("The decision could not be displayed safely.")]
pub struct RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserError {
    WrongGuild,
    Cooldown,
    Busy,
    Authentication,
    InvalidRequest,
    RateLimited,
    Overloaded,
    Timeout,
    InvalidResponse,
    Transport,
    Display,
    Internal,
}

pub fn decision(
    request: &DecisionRequest,
    result: &DecisionResult,
    language: Language,
) -> Result<RenderedDecision, RenderError> {
    let selected = request
        .options()
        .iter()
        .find(|option| option.id() == result.selected_id)
        .ok_or(RenderError)?;
    if result.probabilities.len() != request.options().len() {
        return Err(RenderError);
    }

    let mut fields = vec![RenderedField {
        name: localized(language, "Question", "質問").into(),
        value: escape_markdown(request.question()),
    }];
    if let Some(context) = request.context() {
        fields.push(RenderedField {
            name: localized(language, "Context", "補足").into(),
            value: escape_markdown(context),
        });
    }
    for option in request.options() {
        let probability = result.probabilities.get(&option.id()).ok_or(RenderError)?;
        if !probability.is_finite() || !(0.0..=1.0).contains(probability) {
            return Err(RenderError);
        }
        let marker = if option.id() == result.selected_id {
            localized(language, " (selected)", "（選択）")
        } else {
            ""
        };
        fields.push(RenderedField {
            name: format!("{} · {:.1}%{marker}", option.id(), probability * 100.0),
            value: escape_markdown(option.text()),
        });
    }

    // Provider metadata must not add arbitrary lines to the footer.
    let compact_model = result
        .model
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let model = truncate_model(&compact_model);
    let rendered = RenderedDecision {
        title: localized(language, "Jev's choice", "Jevの回答").into(),
        description: format!("{}. {}", selected.id(), escape_markdown(selected.text())),
        fields,
        footer: format!(
            "{}: {}\n{}",
            localized(language, "Model", "モデル"),
            model,
            localized(
                language,
                "Probabilities are model assessments, not accuracy scores.",
                "確率はモデルの評価であり、正解率ではありません。",
            ),
        ),
    };
    rendered.check_limits()?;
    Ok(rendered)
}

impl RenderedDecision {
    fn check_limits(&self) -> Result<(), RenderError> {
        let units = |value: &str| value.encode_utf16().count();
        if units(&self.title) > 256
            || units(&self.description) > 4096
            || units(&self.footer) > 2048
            || self.fields.len() > 25
        {
            return Err(RenderError);
        }
        let mut total = units(&self.title) + units(&self.description) + units(&self.footer);
        for field in &self.fields {
            let name = units(&field.name);
            let value = units(&field.value);
            if name == 0 || value == 0 || name > 256 || value > 1024 {
                return Err(RenderError);
            }
            total += name + value;
        }
        if total > 6000 {
            return Err(RenderError);
        }
        Ok(())
    }
}

fn escape_markdown(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut line_prefix = LinePrefix::Whitespace;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        let is_ordered_list_period = character == '.'
            && line_prefix == LinePrefix::Digits
            && matches!(characters.peek(), None | Some(' ' | '\t' | '\n'));
        if matches!(
            character,
            '\\' | '`'
                | '*'
                | '_'
                | '~'
                | '|'
                | '['
                | ']'
                | '('
                | ')'
                | '<'
                | '>'
                | '#'
                | '-'
                | '+'
        ) || is_ordered_list_period
        {
            escaped.push('\\');
        }
        escaped.push(character);
        line_prefix = match (line_prefix, character) {
            (_, '\n') => LinePrefix::Whitespace,
            (LinePrefix::Whitespace, ' ' | '\t') => LinePrefix::Whitespace,
            (LinePrefix::Whitespace | LinePrefix::Digits, character)
                if character.is_ascii_digit() =>
            {
                LinePrefix::Digits
            }
            _ => LinePrefix::Other,
        };
    }
    escaped
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinePrefix {
    Whitespace,
    Digits,
    Other,
}

fn truncate_model(model: &str) -> String {
    if model.chars().count() <= 80 {
        model.to_owned()
    } else {
        model.chars().take(79).chain(std::iter::once('…')).collect()
    }
}

fn localized<'a>(language: Language, english: &'a str, japanese: &'a str) -> &'a str {
    match language {
        Language::English => english,
        Language::Japanese => japanese,
    }
}

fn field_name(field: InputField, language: Language) -> String {
    match field {
        InputField::Question => localized(language, "Question", "質問").into(),
        InputField::Context => localized(language, "Context", "補足").into(),
        InputField::Option(id) => format!("{}{id}", localized(language, "Option ", "選択肢")),
    }
}

pub fn input_error(error: &InputError, language: Language) -> String {
    match error {
        InputError::Empty { field } => {
            let name = field_name(*field, language);
            match language {
                Language::English => {
                    format!("{name} cannot be blank. Enter text or omit an optional choice.")
                }
                Language::Japanese => {
                    format!("{name}が空です。内容を入力するか、任意の選択肢なら省略してください。")
                }
            }
        }
        InputError::TooLong { field, limit } => {
            let name = field_name(*field, language);
            match language {
                Language::English => format!("{name} must be at most {limit} characters."),
                Language::Japanese => format!("{name}は{limit}文字以内で入力してください。"),
            }
        }
        InputError::MissingPrecedingOption { option, required } => match language {
            Language::English => format!("Option {option} requires option {required}."),
            Language::Japanese => {
                format!("選択肢{option}を指定する場合は、選択肢{required}も入力してください。")
            }
        },
        InputError::DuplicateOptions { first, second } => match language {
            Language::English => {
                format!("Options {first} and {second} have the same text. Enter distinct choices.")
            }
            Language::Japanese => format!(
                "選択肢{first}と{second}の内容が重複しています。異なる内容を入力してください。"
            ),
        },
    }
}

pub fn error(error: UserError, language: Language) -> String {
    let (english, japanese) = match error {
        UserError::WrongGuild => (
            "This command is available only in the configured server.",
            "このコマンドは設定されたサーバーでのみ利用できます。",
        ),
        UserError::Cooldown => (
            "Please wait a few seconds before asking another question.",
            "続けて質問する場合は、少し間隔を空けてください。",
        ),
        UserError::Busy => (
            "Other questions are being processed. Please try again shortly.",
            "現在ほかの質問を処理しています。少し後でお試しください。",
        ),
        UserError::Authentication => (
            "There is a problem with the Jev connection settings. Contact the bot administrator.",
            "Jevとの接続設定に問題があります。Bot管理者に連絡してください。",
        ),
        UserError::InvalidRequest => (
            "Jev could not process this request. Contact the bot administrator.",
            "Jevへのリクエストを処理できませんでした。Bot管理者に連絡してください。",
        ),
        UserError::RateLimited => (
            "Jev's usage limit has been reached. Please try again later.",
            "Jevの利用制限に達しました。少し後でお試しください。",
        ),
        UserError::Overloaded => (
            "Jev is busy. Please try again later.",
            "Jevが混雑しています。少し後でお試しください。",
        ),
        UserError::Timeout => (
            "Jev did not respond in time.",
            "Jevから時間内に回答を受け取れませんでした。",
        ),
        UserError::InvalidResponse => (
            "Jev returned an unexpected response. No choice has been displayed.",
            "Jevから想定外の回答を受け取りました。選択結果は表示していません。",
        ),
        UserError::Transport => (
            "Communication with Jev failed. Please try again later.",
            "Jevとの通信に失敗しました。少し後でお試しください。",
        ),
        UserError::Display => (
            "The result could not be displayed. Contact the bot administrator.",
            "回答を表示できませんでした。Bot管理者に連絡してください。",
        ),
        UserError::Internal => (
            "The command could not be completed. Contact the bot administrator.",
            "コマンドを完了できませんでした。Bot管理者に連絡してください。",
        ),
    };
    localized(language, english, japanese).into()
}
