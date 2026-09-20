use std::str::FromStr;

use jev_pick::decision::{DecisionRequest, InputError, InputField, OptionId, RawDecisionInput};

fn raw() -> RawDecisionInput {
    RawDecisionInput {
        question: "Choose one".into(),
        a: "Alpha".into(),
        b: "Beta".into(),
        c: None,
        d: None,
        context: None,
    }
}

#[test]
fn option_ids_have_stable_text_and_ordering() {
    assert_eq!(OptionId::A.to_string(), "A");
    assert_eq!(OptionId::from_str("D"), Ok(OptionId::D));
    assert!(OptionId::A < OptionId::B);
    assert!(OptionId::from_str("a").is_err());
}

#[test]
fn inputs_are_normalized_without_display_escaping() {
    let request = DecisionRequest::try_from(RawDecisionInput {
        question: "  Pick\r\none\rnow  ".into(),
        a: "  **A**\r\nline  ".into(),
        b: " B ".into(),
        c: None,
        d: None,
        context: Some("  extra\rcontext  ".into()),
    })
    .unwrap();

    assert_eq!(request.question(), "Pick\none\nnow");
    assert_eq!(request.context(), Some("extra\ncontext"));
    assert_eq!(request.options()[0].text(), "**A**\nline");
}

#[test]
fn two_three_and_four_options_keep_contiguous_ids() {
    for (c, d, expected) in [
        (None, None, vec![OptionId::A, OptionId::B]),
        (
            Some("Gamma"),
            None,
            vec![OptionId::A, OptionId::B, OptionId::C],
        ),
        (
            Some("Gamma"),
            Some("Delta"),
            vec![OptionId::A, OptionId::B, OptionId::C, OptionId::D],
        ),
    ] {
        let mut input = raw();
        input.c = c.map(str::to_owned);
        input.d = d.map(str::to_owned);

        let request = DecisionRequest::try_from(input).unwrap();
        let ids: Vec<_> = request.options().iter().map(|option| option.id()).collect();
        assert_eq!(ids, expected);
    }
}

#[test]
fn required_and_present_optional_fields_cannot_be_blank() {
    let cases = [
        (
            {
                let mut input = raw();
                input.question = " \r\n ".into();
                input
            },
            InputField::Question,
        ),
        (
            {
                let mut input = raw();
                input.a = " ".into();
                input
            },
            InputField::Option(OptionId::A),
        ),
        (
            {
                let mut input = raw();
                input.b = "\r".into();
                input
            },
            InputField::Option(OptionId::B),
        ),
        (
            {
                let mut input = raw();
                input.c = Some("\n ".into());
                input
            },
            InputField::Option(OptionId::C),
        ),
        (
            {
                let mut input = raw();
                input.c = Some("Gamma".into());
                input.d = Some("  ".into());
                input
            },
            InputField::Option(OptionId::D),
        ),
    ];

    for (input, field) in cases {
        assert_eq!(
            DecisionRequest::try_from(input),
            Err(InputError::Empty { field })
        );
    }
}

#[test]
fn blank_context_is_absent() {
    let mut input = raw();
    input.context = Some(" \r\n\t ".into());

    let request = DecisionRequest::try_from(input).unwrap();

    assert_eq!(request.context(), None);
}

#[test]
fn option_d_requires_option_c_without_relabeling() {
    let mut input = raw();
    input.d = Some("Delta".into());

    assert_eq!(
        DecisionRequest::try_from(input),
        Err(InputError::MissingPrecedingOption {
            option: OptionId::D,
            required: OptionId::C,
        })
    );
}

#[test]
fn exact_normalized_duplicates_are_rejected() {
    let mut input = raw();
    input.a = " same\r\nvalue ".into();
    input.b = "same\nvalue".into();

    assert_eq!(
        DecisionRequest::try_from(input),
        Err(InputError::DuplicateOptions {
            first: OptionId::A,
            second: OptionId::B,
        })
    );
}

#[test]
fn case_width_and_unicode_normalization_differences_remain_distinct() {
    for (a, b) in [("Choice", "choice"), ("Ａ", "A"), ("é", "e\u{301}")] {
        let mut input = raw();
        input.a = a.into();
        input.b = b.into();
        assert!(DecisionRequest::try_from(input).is_ok());
    }
}

#[test]
fn limits_count_unicode_scalars_after_normalization() {
    let mut input = raw();
    input.question = format!(" {} ", "😀".repeat(300));
    input.a = "界".repeat(120);
    input.context = Some("文".repeat(500));
    assert!(DecisionRequest::try_from(input).is_ok());

    let mut question_too_long = raw();
    question_too_long.question = "😀".repeat(301);
    assert_eq!(
        DecisionRequest::try_from(question_too_long),
        Err(InputError::TooLong {
            field: InputField::Question,
            limit: 300,
        })
    );

    let mut option_too_long = raw();
    option_too_long.a = "界".repeat(121);
    assert_eq!(
        DecisionRequest::try_from(option_too_long),
        Err(InputError::TooLong {
            field: InputField::Option(OptionId::A),
            limit: 120,
        })
    );

    let mut context_too_long = raw();
    context_too_long.context = Some("文".repeat(501));
    assert_eq!(
        DecisionRequest::try_from(context_too_long),
        Err(InputError::TooLong {
            field: InputField::Context,
            limit: 500,
        })
    );
}

#[test]
fn input_errors_do_not_expose_rejected_text() {
    let secret = "private-question-value";
    let mut input = raw();
    input.question = secret.repeat(31);

    let error = DecisionRequest::try_from(input).unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}
