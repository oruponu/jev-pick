use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::decision::{DecisionRequest, DecisionResult, OptionId, TokenUsage};
use crate::language::Language;

use super::JevError;

const SUM_TOLERANCE: f64 = 1e-6;
const CHOICE_TOLERANCE: f64 = 1e-9;

#[derive(Serialize)]
struct RequestDto<'a> {
    model: &'a str,
    state: StateDto<'a>,
    questions: QuestionsDto<'a>,
}

#[derive(Serialize)]
struct StateDto<'a> {
    context: &'a str,
}

#[derive(Serialize)]
struct QuestionsDto<'a> {
    decision: QuestionDto<'a>,
}

#[derive(Serialize)]
struct QuestionDto<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'a str,
    criteria: BTreeMap<String, &'a str>,
}

#[derive(Deserialize)]
struct ResponseDto {
    model: String,
    answers: AnswersDto,
    usage: UsageDto,
}

#[derive(Deserialize)]
struct AnswersDto {
    decision: AnswerDto,
}

#[derive(Deserialize)]
struct AnswerDto {
    #[serde(rename = "type")]
    kind: String,
    choice: String,
    probabilities: BTreeMap<String, f64>,
    confidence: f64,
}

#[derive(Deserialize)]
struct UsageDto {
    input_tokens: u64,
    output_tokens: u64,
}

pub(super) fn serialize_request(
    request: &DecisionRequest,
    model: &str,
) -> Result<Vec<u8>, JevError> {
    let context = request
        .context()
        .unwrap_or_else(|| Language::from_question(request.question()).no_context());
    let criteria = request
        .options()
        .iter()
        .map(|option| (option.id().to_string(), option.text()))
        .collect();
    let dto = RequestDto {
        model,
        state: StateDto { context },
        questions: QuestionsDto {
            decision: QuestionDto {
                kind: "choice",
                instructions: request.question(),
                criteria,
            },
        },
    };
    serde_json::to_vec(&dto).map_err(|_| JevError::InvalidRequest)
}

pub(super) fn parse_response(
    bytes: &[u8],
    request: &DecisionRequest,
) -> Result<DecisionResult, JevError> {
    let response: ResponseDto =
        serde_json::from_slice(bytes).map_err(|_| JevError::InvalidResponse)?;
    if response.model.trim().is_empty()
        || response.answers.decision.kind != "choice"
        || !in_unit_interval(response.answers.decision.confidence)
    {
        return Err(JevError::InvalidResponse);
    }

    let expected: BTreeSet<OptionId> = request.options().iter().map(|option| option.id()).collect();
    let selected_id = response
        .answers
        .decision
        .choice
        .parse::<OptionId>()
        .map_err(|_| JevError::InvalidResponse)?;
    if !expected.contains(&selected_id) {
        return Err(JevError::InvalidResponse);
    }

    let mut probabilities = BTreeMap::new();
    for (raw_id, probability) in response.answers.decision.probabilities {
        let id = raw_id
            .parse::<OptionId>()
            .map_err(|_| JevError::InvalidResponse)?;
        if !in_unit_interval(probability) || probabilities.insert(id, probability).is_some() {
            return Err(JevError::InvalidResponse);
        }
    }
    if probabilities.keys().copied().collect::<BTreeSet<_>>() != expected {
        return Err(JevError::InvalidResponse);
    }

    let sum: f64 = probabilities.values().sum();
    if !sum.is_finite() || (sum - 1.0).abs() > SUM_TOLERANCE {
        return Err(JevError::InvalidResponse);
    }
    let selected_probability = probabilities[&selected_id];
    let maximum = probabilities
        .values()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if maximum - selected_probability > CHOICE_TOLERANCE {
        return Err(JevError::InvalidResponse);
    }

    Ok(DecisionResult {
        selected_id,
        probabilities,
        confidence: response.answers.decision.confidence,
        model: response.model,
        usage: TokenUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        },
    })
}

fn in_unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
