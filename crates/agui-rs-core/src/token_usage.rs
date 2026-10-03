//! Token-usage adapter functions, ported from the official AG-UI 1.0.0
//! TypeScript SDK (`sdks/typescript/packages/core/src/token-usage.ts`,
//! commit `024332cbb71e03e6a6bc055bed5af9c5c504471a`).
//!
//! The two vendor mappers take an *untrusted* vendor payload as a
//! [`serde_json::Value`] — the Rust counterpart of the upstream `unknown`
//! parameter, since Rust has no AI-SDK/LangChain types. A malformed count is
//! rejected (never reaches the wire) and warned about once per key per
//! process, exactly as upstream.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::events::TokenUsage;

/// What the value WAS, said in a way that points at the provider's bug.
/// Mirrors upstream `describeRejected` (token-usage.ts:22). Arrays report as
/// objects because `typeof [] === "object"`; the non-finite-number branch is
/// unreachable here because `serde_json::Number` cannot hold NaN/∞.
fn describe_rejected(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::String(s) => format!(
            "{} (a string)",
            serde_json::to_string(s).unwrap_or_default()
        ),
        Value::Number(_) => "a non-finite number".to_string(),
        Value::Bool(b) => format!("{} (a boolean)", b),
        Value::Object(_) | Value::Array(_) => "an object".to_string(),
    }
}

/// The once-per-key-per-process suppression set (upstream `warnedCountKeys`,
/// token-usage.ts:19): a provider that hands over a string count hands one
/// over on every call, and a per-call warning would bury the stream.
fn warn_once(key: &'static str, value: &Value) {
    static WARNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let already = WARNED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap()
        .insert(key);
    if !already {
        return;
    }
    eprintln!(
        "[ag-ui] usage.{} was {}, not a number — omitted from this run's usage. Reported once per key.",
        key,
        describe_rejected(value)
    );
}

/// Read a property from a value of unknown shape (upstream `prop`,
/// token-usage.ts:71): `None` for anything that is not an object.
fn prop<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object().and_then(|o| o.get(key))
}

/// Accept a value only if it is a real, non-negative integer (upstream `num`,
/// token-usage.ts:53 — our `TokenUsage` counts are `u64`, matching the schema's
/// `integer`/`minimum: 0`). An absent key is how a provider spells "did not
/// report this count" and draws no warning; anything else non-numeric
/// (string, null, bool, object) warns once per key and is dropped.
fn num(value: Option<&Value>, key: &'static str) -> Option<u64> {
    let v = value?;
    match v.as_u64() {
        Some(n) => Some(n),
        None => {
            warn_once(key, v);
            None
        }
    }
}

/// Build a [`TokenUsage`] from already-guarded counts, or `None` when no count
/// survived (upstream `buildEntry`, token-usage.ts:80). Returning `None` keeps
/// "the provider reported no usage" distinct from "the provider reported
/// usage".
fn build_entry(
    counts: [Option<u64>; 6],
    provider: Option<&str>,
    model: Option<&str>,
) -> Option<TokenUsage> {
    let [input_tokens, output_tokens, total_tokens, reasoning_tokens, cached_input_tokens, cache_write_input_tokens] =
        counts;
    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && reasoning_tokens.is_none()
        && cached_input_tokens.is_none()
        && cache_write_input_tokens.is_none()
    {
        return None;
    }
    Some(TokenUsage {
        provider: provider.map(str::to_owned),
        model: model.map(str::to_owned),
        input_tokens,
        output_tokens,
        total_tokens,
        reasoning_tokens,
        cached_input_tokens,
        cache_write_input_tokens,
    })
}

/// Map a LangChain-family `usage_metadata` object into an AG-UI
/// [`TokenUsage`] (upstream `tokenUsageFromLangChainMetadata`,
/// token-usage.ts:111). LangChain's accounting is the protocol's —
/// `input_tokens` already includes the cache details and `output_tokens` the
/// reasoning detail — so every count passes through as is. Returns `None`
/// when no usable count is present.
pub fn token_usage_from_lang_chain_metadata(
    usage_metadata: &Value,
    provider: Option<&str>,
    model: Option<&str>,
) -> Option<TokenUsage> {
    if usage_metadata.is_null() {
        return None;
    }
    let input_details = prop(usage_metadata, "input_token_details");
    let output_details = prop(usage_metadata, "output_token_details");

    build_entry(
        [
            num(prop(usage_metadata, "input_tokens"), "inputTokens"),
            num(prop(usage_metadata, "output_tokens"), "outputTokens"),
            num(prop(usage_metadata, "total_tokens"), "totalTokens"),
            num(
                output_details.and_then(|d| prop(d, "reasoning")),
                "reasoningTokens",
            ),
            num(
                input_details.and_then(|d| prop(d, "cache_read")),
                "cachedInputTokens",
            ),
            num(
                input_details.and_then(|d| prop(d, "cache_creation")),
                "cacheWriteInputTokens",
            ),
        ],
        provider,
        model,
    )
}

/// Map an AI-SDK `LanguageModelUsage` object into an AG-UI [`TokenUsage`]
/// (upstream `tokenUsageFromAiSdkUsage`, token-usage.ts:146). The v5 keys
/// already match; v6 adds `inputTokenDetails`/`outputTokenDetails`, and the
/// two counts present in both forms are read from the top level first.
/// Returns `None` when no finite count is present.
pub fn token_usage_from_ai_sdk_usage(
    usage: &Value,
    provider: Option<&str>,
    model: Option<&str>,
) -> Option<TokenUsage> {
    if usage.is_null() {
        return None;
    }
    let input_details = prop(usage, "inputTokenDetails");
    let output_details = prop(usage, "outputTokenDetails");

    build_entry(
        [
            num(prop(usage, "inputTokens"), "inputTokens"),
            num(prop(usage, "outputTokens"), "outputTokens"),
            num(prop(usage, "totalTokens"), "totalTokens"),
            num(prop(usage, "reasoningTokens"), "reasoningTokens").or_else(|| {
                num(
                    output_details.and_then(|d| prop(d, "reasoningTokens")),
                    "reasoningTokens",
                )
            }),
            num(prop(usage, "cachedInputTokens"), "cachedInputTokens").or_else(|| {
                num(
                    input_details.and_then(|d| prop(d, "cacheReadTokens")),
                    "cachedInputTokens",
                )
            }),
            num(
                input_details.and_then(|d| prop(d, "cacheWriteTokens")),
                "cacheWriteInputTokens",
            ),
        ],
        provider,
        model,
    )
}

/// Sum one optional count into a group accumulator; stays `None` when no
/// member of the group reported it, so "not reported" stays distinct from
/// zero (upstream aggregateTokenUsage, token-usage.ts:190-194).
fn add_count(target: &mut Option<u64>, value: &Option<u64>) {
    if let Some(v) = value {
        *target = Some(target.unwrap_or(0) + v);
    }
}

/// Sum per-call [`TokenUsage`] entries into one entry per `(provider, model)`
/// pair, in first-appearance order (upstream `aggregateTokenUsage`,
/// token-usage.ts:180). Each of the six counts is summed independently —
/// `reasoningTokens`/`cachedInputTokens`/`cacheWriteInputTokens` are *parts*
/// of the totals, never additions, so summing every field never
/// double-counts. A count stays `None` when no member of the group reported
/// it.
pub fn aggregate_token_usage(entries: &[TokenUsage]) -> Vec<TokenUsage> {
    // ponytail: linear scan over groups; group counts are tiny (one per
    // provider/model pair). Switch to a keyed map if that ever matters.
    let mut groups: Vec<TokenUsage> = Vec::new();
    for entry in entries {
        let target = if let Some(g) = groups
            .iter_mut()
            .find(|g| g.provider == entry.provider && g.model == entry.model)
        {
            g
        } else {
            groups.push(TokenUsage {
                provider: entry.provider.clone(),
                model: entry.model.clone(),
                ..Default::default()
            });
            groups.last_mut().expect("just pushed")
        };
        add_count(&mut target.input_tokens, &entry.input_tokens);
        add_count(&mut target.output_tokens, &entry.output_tokens);
        add_count(&mut target.total_tokens, &entry.total_tokens);
        add_count(&mut target.reasoning_tokens, &entry.reasoning_tokens);
        add_count(&mut target.cached_input_tokens, &entry.cached_input_tokens);
        add_count(
            &mut target.cache_write_input_tokens,
            &entry.cache_write_input_tokens,
        );
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ai_sdk_v5_keys_already_match() {
        let u = token_usage_from_ai_sdk_usage(
            &json!({
                "inputTokens": 100, "outputTokens": 50, "totalTokens": 150,
                "reasoningTokens": 20, "cachedInputTokens": 10
            }),
            Some("openai"),
            Some("gpt-4o"),
        )
        .unwrap();
        assert_eq!(
            u,
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
                reasoning_tokens: Some(20),
                cached_input_tokens: Some(10),
                cache_write_input_tokens: None,
            }
        );
    }

    #[test]
    fn ai_sdk_v6_details_where_cache_write_lives() {
        let u = token_usage_from_ai_sdk_usage(
            &json!({
                "inputTokens": 100, "outputTokens": 50, "totalTokens": 150,
                "inputTokenDetails": {"noCacheTokens": 70, "cacheReadTokens": 20, "cacheWriteTokens": 10},
                "outputTokenDetails": {"textTokens": 45, "reasoningTokens": 5}
            }),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            u,
            TokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
                reasoning_tokens: Some(5),
                cached_input_tokens: Some(20),
                cache_write_input_tokens: Some(10),
                ..Default::default()
            }
        );
    }

    #[test]
    fn ai_sdk_prefers_v5_top_level_when_both_forms_present() {
        let u = token_usage_from_ai_sdk_usage(
            &json!({
                "inputTokens": 10, "reasoningTokens": 3, "cachedInputTokens": 4,
                "inputTokenDetails": {"cacheReadTokens": 999},
                "outputTokenDetails": {"reasoningTokens": 999}
            }),
            None,
            None,
        )
        .unwrap();
        assert_eq!(u.reasoning_tokens, Some(3));
        assert_eq!(u.cached_input_tokens, Some(4));
        assert_eq!(u.cache_write_input_tokens, None);
    }

    #[test]
    fn ai_sdk_absent_counts_omitted_and_empty_returns_none() {
        let u = token_usage_from_ai_sdk_usage(
            &json!({"inputTokens": 12, "outputTokens": null}),
            None,
            None,
        );
        // `null` is a defect (warned, dropped), not an absent key — but the
        // surviving inputTokens still builds an entry.
        assert_eq!(u.unwrap().input_tokens, Some(12));
        assert!(token_usage_from_ai_sdk_usage(&json!({}), None, None).is_none());
        assert!(token_usage_from_ai_sdk_usage(&Value::Null, None, None).is_none());
    }

    #[test]
    fn lang_chain_maps_core_and_detail_fields() {
        let u = token_usage_from_lang_chain_metadata(
            &json!({
                "input_tokens": 100, "output_tokens": 50, "total_tokens": 150,
                "input_token_details": {"cache_read": 10, "cache_creation": 5},
                "output_token_details": {"reasoning": 20}
            }),
            Some("anthropic"),
            Some("claude-sonnet-4"),
        )
        .unwrap();
        assert_eq!(
            u,
            TokenUsage {
                provider: Some("anthropic".into()),
                model: Some("claude-sonnet-4".into()),
                input_tokens: Some(100),
                output_tokens: Some(50),
                total_tokens: Some(150),
                reasoning_tokens: Some(20),
                cached_input_tokens: Some(10),
                cache_write_input_tokens: Some(5),
            }
        );
    }

    #[test]
    fn lang_chain_null_and_empty_return_none_and_absent_fields_omitted() {
        assert!(token_usage_from_lang_chain_metadata(&Value::Null, None, None).is_none());
        assert!(token_usage_from_lang_chain_metadata(&json!({}), None, None).is_none());
        let u = token_usage_from_lang_chain_metadata(&json!({"input_tokens": 5}), None, None);
        assert_eq!(
            u.unwrap(),
            TokenUsage {
                input_tokens: Some(5),
                ..Default::default()
            }
        );
    }

    #[test]
    fn aggregate_sums_same_provider_model() {
        let agg = aggregate_token_usage(&[
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                input_tokens: Some(100),
                output_tokens: Some(20),
                total_tokens: Some(120),
                ..Default::default()
            },
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                input_tokens: Some(10),
                output_tokens: Some(5),
                total_tokens: Some(15),
                ..Default::default()
            },
        ]);
        assert_eq!(agg.len(), 1);
        assert_eq!(agg[0].input_tokens, Some(110));
        assert_eq!(agg[0].output_tokens, Some(25));
        assert_eq!(agg[0].total_tokens, Some(135));
    }

    #[test]
    fn aggregate_keeps_models_separate_in_first_seen_order() {
        let agg = aggregate_token_usage(&[
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                input_tokens: Some(1),
                ..Default::default()
            },
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o-mini".into()),
                input_tokens: Some(2),
                ..Default::default()
            },
            TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                input_tokens: Some(3),
                ..Default::default()
            },
        ]);
        assert_eq!(
            agg.iter().map(|u| u.model.clone()).collect::<Vec<_>>(),
            vec![Some("gpt-4o".into()), Some("gpt-4o-mini".into())]
        );
        assert_eq!(agg[0].input_tokens, Some(4));
        assert_eq!(agg[1].input_tokens, Some(2));
    }

    #[test]
    fn aggregate_empty_input_yields_empty() {
        assert!(aggregate_token_usage(&[]).is_empty());
    }

    #[test]
    fn aggregate_sums_cache_breakdown_like_every_other_count() {
        let agg = aggregate_token_usage(&[
            TokenUsage {
                provider: Some("p".into()),
                model: Some("m".into()),
                input_tokens: Some(10),
                cached_input_tokens: Some(4),
                cache_write_input_tokens: Some(2),
                ..Default::default()
            },
            TokenUsage {
                provider: Some("p".into()),
                model: Some("m".into()),
                input_tokens: Some(20),
                cached_input_tokens: Some(6),
                cache_write_input_tokens: Some(3),
                ..Default::default()
            },
        ]);
        assert_eq!(agg[0].input_tokens, Some(30));
        assert_eq!(agg[0].cached_input_tokens, Some(10));
        assert_eq!(agg[0].cache_write_input_tokens, Some(5));
    }

    #[test]
    fn aggregate_leaves_unreported_count_none_not_zero() {
        let agg = aggregate_token_usage(&[
            TokenUsage {
                provider: Some("p".into()),
                model: Some("m".into()),
                input_tokens: Some(1),
                ..Default::default()
            },
            TokenUsage {
                provider: Some("p".into()),
                model: Some("m".into()),
                input_tokens: Some(2),
                ..Default::default()
            },
        ]);
        assert_eq!(agg[0].input_tokens, Some(3));
        assert_eq!(agg[0].output_tokens, None);
    }

    #[test]
    fn rejected_counts_are_dropped_and_described_by_ag_ui_key() {
        // Upstream token-usage-warnings.test.ts: a string count is dropped and
        // named by its AG-UI key, not the vendor's spelling.
        let u = token_usage_from_ai_sdk_usage(&json!({"inputTokens": "1024"}), None, None);
        assert!(u.is_none());

        let msg = describe_rejected(&json!("1024"));
        assert_eq!(msg, "\"1024\" (a string)");
        assert_eq!(describe_rejected(&Value::Null), "null");
        assert_eq!(describe_rejected(&json!({})), "an object");
        assert_eq!(describe_rejected(&json!(true)), "true (a boolean)");
    }

    #[test]
    fn absent_count_draws_no_warning() {
        // num() on an absent key returns None without touching the warn path.
        let u = token_usage_from_ai_sdk_usage(&json!({"inputTokens": 5}), None, None).unwrap();
        assert_eq!(u.input_tokens, Some(5));
        assert_eq!(u.output_tokens, None);
    }
}
