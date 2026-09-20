use std::collections::BTreeMap;

use jev_pick::{
    decision::{
        DecisionRequest, DecisionResult, InputError, InputField, OptionId, RawDecisionInput,
        TokenUsage,
    },
    language::Language,
    render::{self, UserError},
};

fn input() -> RawDecisionInput {
    RawDecisionInput {
        question: "What should I eat?".into(),
        a: "Curry".into(),
        b: "Udon".into(),
        c: Some("Sushi".into()),
        d: None,
        context: Some("Something warm".into()),
    }
}

fn answer() -> DecisionResult {
    DecisionResult {
        selected_id: OptionId::B,
        probabilities: BTreeMap::from([
            (OptionId::C, 0.05),
            (OptionId::B, 0.85),
            (OptionId::A, 0.10),
        ]),
        confidence: 0.8,
        model: "jev-test".into(),
        usage: TokenUsage {
            input_tokens: 160,
            output_tokens: 32,
        },
    }
}

#[test]
fn preserves_option_order_original_selected_text_and_percentage_mapping() {
    let request = DecisionRequest::try_from(input()).unwrap();
    let rendered = render::decision(&request, &answer(), Language::English).unwrap();
    assert_eq!(rendered.title, "Jev's choice");
    assert_eq!(rendered.description, "B. Udon");
    assert_eq!(rendered.fields[0].value, "What should I eat?");
    assert_eq!(rendered.fields[1].value, "Something warm");
    assert_eq!(rendered.fields[2].name, "A · 10.0%");
    assert_eq!(rendered.fields[3].name, "B · 85.0% (selected)");
    assert_eq!(rendered.fields[4].name, "C · 5.0%");
    assert_eq!(rendered.fields[4].value, "Sushi");
    assert!(!rendered.footer.contains("80.0"));
    assert!(rendered.footer.contains("jev-test"));
    assert!(rendered.footer.contains("not accuracy"));
}

#[test]
fn japanese_headings_do_not_translate_the_original_input() {
    let request = DecisionRequest::try_from(input()).unwrap();
    let rendered = render::decision(&request, &answer(), Language::Japanese).unwrap();
    assert_eq!(rendered.title, "Jevの回答");
    assert_eq!(rendered.description, "B. Udon");
    assert_eq!(rendered.fields[0].name, "質問");
    assert_eq!(rendered.fields[0].value, "What should I eat?");
    assert!(rendered.fields[3].name.contains("選択"));
    assert!(rendered.footer.contains("正解率ではありません"));
}

#[test]
fn omits_absent_context_and_does_not_rebalance_rounded_percentages() {
    let mut raw = input();
    raw.context = None;
    let request = DecisionRequest::try_from(raw).unwrap();
    let mut result = answer();
    result
        .probabilities
        .values_mut()
        .for_each(|value| *value = 1.0 / 3.0);
    let rendered = render::decision(&request, &result, Language::English).unwrap();
    assert_eq!(rendered.fields.len(), 4);
    assert!(
        rendered.fields[1..]
            .iter()
            .all(|field| field.name.contains("33.3%"))
    );
}

#[test]
fn escapes_display_markdown_but_never_changes_the_request() {
    let mut raw = input();
    raw.a = "**bold** [link](https://example.com)".into();
    raw.b = "```\n@everyone <@123> <@&456>\n```".into();
    raw.question = "> quote\n# heading\n- item\n_italic_ |spoiler| \\".into();
    let request = DecisionRequest::try_from(raw).unwrap();
    let rendered = render::decision(&request, &answer(), Language::English).unwrap();
    assert_eq!(
        rendered.fields[2].value,
        "\\*\\*bold\\*\\* \\[link\\]\\(https://example.com\\)"
    );
    assert!(rendered.description.contains("\\`\\`\\`"));
    assert!(rendered.description.contains("\\<@123\\>"));
    assert!(rendered.description.contains("@everyone"));
    assert!(
        rendered.fields[0]
            .value
            .starts_with("\\> quote\n\\# heading\n\\- item")
    );
    assert_eq!(
        request.options()[0].text(),
        "**bold** [link](https://example.com)"
    );
}

#[test]
fn escapes_indented_ordered_lists_without_escaping_url_periods_or_changing_input() {
    let mut raw = input();
    raw.question = "Question\n  1. first\nhttps://example.com/v1.2".into();
    raw.context = Some("Context\n  2. second".into());
    raw.a = "Alpha\n  3. third".into();
    raw.b = "Beta\n  4. fourth".into();
    let request = DecisionRequest::try_from(raw).unwrap();

    let rendered = render::decision(&request, &answer(), Language::English).unwrap();

    assert_eq!(
        rendered.fields[0].value,
        "Question\n  1\\. first\nhttps://example.com/v1.2"
    );
    assert_eq!(rendered.fields[1].value, "Context\n  2\\. second");
    assert_eq!(rendered.fields[2].value, "Alpha\n  3\\. third");
    assert_eq!(rendered.fields[3].value, "Beta\n  4\\. fourth");
    assert_eq!(rendered.description, "B. Beta\n  4\\. fourth");

    assert_eq!(
        request.question(),
        "Question\n  1. first\nhttps://example.com/v1.2"
    );
    assert_eq!(request.context(), Some("Context\n  2. second"));
    assert_eq!(request.options()[0].text(), "Alpha\n  3. third");
    assert_eq!(request.options()[1].text(), "Beta\n  4. fourth");
}

fn assert_limits(rendered: &render::RenderedDecision) {
    let units = |text: &str| text.encode_utf16().count();
    assert!(units(&rendered.title) <= 256);
    assert!(units(&rendered.description) <= 4096);
    assert!(units(&rendered.footer) <= 2048);
    let mut total = units(&rendered.title) + units(&rendered.description) + units(&rendered.footer);
    for field in &rendered.fields {
        assert!(units(&field.name) <= 256);
        assert!(units(&field.value) <= 1024);
        total += units(&field.name) + units(&field.value);
    }
    assert!(total <= 6000);
}

#[test]
fn maximum_length_unicode_and_escape_expansion_fit_without_truncating_input() {
    for character in ["😀", "*", "漢", "\\"] {
        let raw = RawDecisionInput {
            question: character.repeat(300),
            a: format!("A{}", character.repeat(119)),
            b: format!("B{}", character.repeat(119)),
            c: Some(format!("C{}", character.repeat(119))),
            d: Some(format!("D{}", character.repeat(119))),
            context: Some(character.repeat(500)),
        };
        let request = DecisionRequest::try_from(raw).unwrap();
        let mut result = answer();
        result.probabilities = BTreeMap::from([
            (OptionId::A, 0.25),
            (OptionId::B, 0.25),
            (OptionId::C, 0.25),
            (OptionId::D, 0.25),
        ]);
        for language in [Language::English, Language::Japanese] {
            let rendered = render::decision(&request, &result, language).unwrap();
            assert_limits(&rendered);
            assert_eq!(rendered.fields.len(), 6);
            assert!(
                !rendered
                    .fields
                    .iter()
                    .any(|field| field.value.contains('…'))
            );
        }
    }
}

#[test]
fn truncates_only_the_display_model_to_eighty_scalars_including_ellipsis() {
    let request = DecisionRequest::try_from(input()).unwrap();
    let mut result = answer();
    result.model = "界".repeat(81);
    let rendered = render::decision(&request, &result, Language::English).unwrap();
    assert!(
        rendered
            .footer
            .starts_with(&format!("Model: {}…\n", "界".repeat(79)))
    );
    assert_eq!(result.model.chars().count(), 81);
    assert_eq!(rendered.description, "B. Udon");
    assert_limits(&rendered);
}

#[test]
fn rejects_unrenderable_results_instead_of_panicking_or_inventing_values() {
    let request = DecisionRequest::try_from(input()).unwrap();
    let mut result = answer();
    result.selected_id = OptionId::D;
    assert!(render::decision(&request, &result, Language::English).is_err());
    result.selected_id = OptionId::B;
    result.probabilities.remove(&OptionId::A);
    assert!(render::decision(&request, &result, Language::English).is_err());
}

#[test]
fn input_errors_are_localized_without_reflecting_input_text() {
    let cases = [
        InputError::Empty {
            field: InputField::Question,
        },
        InputError::Empty {
            field: InputField::Option(OptionId::C),
        },
        InputError::TooLong {
            field: InputField::Context,
            limit: 500,
        },
        InputError::MissingPrecedingOption {
            option: OptionId::D,
            required: OptionId::C,
        },
        InputError::DuplicateOptions {
            first: OptionId::A,
            second: OptionId::B,
        },
    ];
    for issue in cases {
        let english = render::input_error(&issue, Language::English);
        let japanese = render::input_error(&issue, Language::Japanese);
        assert!(!english.is_empty());
        assert!(!japanese.is_empty());
        assert_ne!(english, japanese);
    }
    assert!(
        render::input_error(
            &InputError::TooLong {
                field: InputField::Question,
                limit: 300
            },
            Language::English
        )
        .contains("300")
    );
}

#[test]
fn all_operational_errors_have_safe_messages_in_both_languages() {
    for issue in [
        UserError::WrongGuild,
        UserError::Cooldown,
        UserError::Busy,
        UserError::Authentication,
        UserError::InvalidRequest,
        UserError::RateLimited,
        UserError::Overloaded,
        UserError::Timeout,
        UserError::InvalidResponse,
        UserError::Transport,
        UserError::Display,
        UserError::Internal,
    ] {
        let english = render::error(issue, Language::English);
        let japanese = render::error(issue, Language::Japanese);
        assert!(!english.is_empty());
        assert!(!japanese.is_empty());
        assert_ne!(english, japanese);
        assert!(!english.contains("https://"));
    }
}
