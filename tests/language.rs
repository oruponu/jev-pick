use jev_pick::language::Language;

#[test]
fn ascii_and_other_non_japanese_text_select_english() {
    assert_eq!(Language::from_question("Which option?"), Language::English);
    assert_eq!(Language::from_question("¿Cuál? 😀"), Language::English);
}

#[test]
fn kana_or_cjk_ideographs_select_japanese_even_in_mixed_text() {
    for question in [
        "ひらがな",
        "カタカナ",
        "半角ｶﾅ",
        "supplementary 𛅐",
        "今日",
        "Choose 寿司",
        "extension 𠀀",
        "newer extension 𰀀",
    ] {
        assert_eq!(Language::from_question(question), Language::Japanese);
    }
}

#[test]
fn extension_j_boundaries_select_japanese() {
    assert_eq!(Language::from_question("\u{323b0}"), Language::Japanese);
    assert_eq!(Language::from_question("\u{33479}"), Language::Japanese);
    assert_eq!(Language::from_question("\u{3347a}"), Language::English);
}

#[test]
fn each_language_has_the_required_context_fallback() {
    assert_eq!(
        Language::English.no_context(),
        "No additional context was provided."
    );
    assert_eq!(
        Language::Japanese.no_context(),
        "追加の条件は指定されていません。"
    );
}
