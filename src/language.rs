#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Japanese,
}

impl Language {
    pub fn from_question(question: &str) -> Self {
        if question.chars().any(is_japanese_script) {
            Self::Japanese
        } else {
            Self::English
        }
    }

    pub fn no_context(self) -> &'static str {
        match self {
            Self::English => "No additional context was provided.",
            Self::Japanese => "追加の条件は指定されていません。",
        }
    }
}

fn is_japanese_script(character: char) -> bool {
    matches!(
        character,
        '\u{3040}'..='\u{30ff}'
            | '\u{31f0}'..='\u{31ff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff66}'..='\u{ff9f}'
            | '\u{1aff0}'..='\u{1afff}'
            | '\u{1b000}'..='\u{1b16f}'
            | '\u{20000}'..='\u{2ee5f}'
            | '\u{2f800}'..='\u{2fa1f}'
            | '\u{30000}'..='\u{33479}'
    )
}
