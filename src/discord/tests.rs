use serde_json::{Value, json};

use super::*;
use crate::render::RenderedField;

fn json_value(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn assert_no_mentions(data: &Value) {
    assert_eq!(
        data["allowed_mentions"],
        json!({
            "parse": [], "users": [], "roles": [], "replied_user": false,
        })
    );
}

#[test]
fn command_registration_contains_only_the_expected_ordered_string_options() {
    let definition = json_value(command_definition().unwrap());
    assert_eq!(definition["name"], "jev");
    assert!(definition.get("contexts").is_none());
    assert!(definition.get("integration_types").is_none());
    let options = definition["options"].as_array().unwrap();
    assert_eq!(options.len(), 6);
    for (index, (name, required, maximum)) in [
        ("question", true, 300),
        ("a", true, 120),
        ("b", true, 120),
        ("c", false, 120),
        ("d", false, 120),
        ("context", false, 500),
    ]
    .into_iter()
    .enumerate()
    {
        let option = &options[index];
        assert_eq!(option["name"], name);
        assert_eq!(option["type"], 3);
        assert_eq!(option["required"], required);
        assert_eq!(option["max_length"], maximum);
        assert!(
            option
                .get("choices")
                .is_none_or(|choices| choices.as_array().unwrap().is_empty())
        );
    }
}

#[test]
fn framework_errors_choose_language_from_the_question_instead_of_other_arguments() {
    for (question, language) in [
        ("Which one?", Language::English),
        ("どちら？", Language::Japanese),
    ] {
        let data: serenity::CommandData = serde_json::from_value(json!({
            "id": "1", "name": "jev", "type": 1,
            "options": [
                {"name": "a", "type": 3, "value": "うどん"},
                {"name": "question", "type": 3, "value": question}
            ]
        }))
        .unwrap();
        assert_eq!(question_language(&data.options()), language);
    }
    assert_eq!(question_language(&[]), Language::English);
}

#[test]
fn command_enables_standard_user_cooldown_without_prefix_or_dm_access() {
    let command = jev();
    assert_eq!(
        command.cooldown_config.read().unwrap().user,
        Some(std::time::Duration::from_secs(10))
    );
    assert!(command.guild_only);
    assert!(command.prefix_action.is_none());
    assert!(command.slash_action.is_some());
}

#[test]
fn public_defer_and_private_errors_explicitly_suppress_mentions() {
    let deferred = json_value(deferred_response());
    assert_eq!(deferred["type"], 5);
    assert_eq!(
        deferred["data"]["flags"].as_u64().unwrap_or_default() & 64,
        0
    );
    assert_no_mentions(&deferred["data"]);
    let private = json_value(private_response("@everyone".into()));
    assert_eq!(private["type"], 4);
    assert_eq!(
        private["data"]["flags"].as_u64().unwrap_or_default() & 64,
        64
    );
    assert_eq!(private["data"]["content"], "@everyone");
    assert_no_mentions(&private["data"]);
}

#[test]
fn edits_preserve_visibility_clear_previous_embeds_and_suppress_mentions() {
    let edit = json_value(error_edit("A safe error".into()));
    assert_eq!(edit["content"], "A safe error");
    assert_eq!(edit["embeds"], json!([]));
    assert!(edit.get("flags").is_none());
    assert_no_mentions(&edit);
}

#[test]
fn result_edit_sends_exactly_one_embed_and_no_message_content_or_mentions() {
    let edit = json_value(result_edit(RenderedDecision {
        title: "Jev's choice".into(),
        description: "B. Udon".into(),
        fields: vec![RenderedField {
            name: "Question".into(),
            value: "Dinner?".into(),
        }],
        footer: "Model: jev-test".into(),
    }));
    assert_eq!(edit["content"], "");
    assert_eq!(edit["embeds"].as_array().unwrap().len(), 1);
    assert_eq!(edit["embeds"][0]["description"], "B. Udon");
    assert_eq!(edit["embeds"][0]["fields"][0]["inline"], false);
    assert_eq!(edit["embeds"][0]["footer"]["text"], "Model: jev-test");
    assert!(edit.get("flags").is_none());
    assert_no_mentions(&edit);
}
