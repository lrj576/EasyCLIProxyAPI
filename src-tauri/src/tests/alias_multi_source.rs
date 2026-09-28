use super::support::*;
use super::*;

const OPENAI_TWO_PROVIDERS: &str = r#"openai-compatibility:
  - name: provider-one
    base-url: https://one.example.com/v1
    api-key-entries:
      - api-key: key-one
    models:
      - name: model-a
      - name: model-b
  - name: provider-two
    base-url: https://two.example.com/v1
    api-key-entries:
      - api-key: key-two
    models:
      - name: model-c
"#;

const CODEX_TWO_PROVIDERS: &str = r#"codex-api-key:
  - name: codex-one
    api-key: key-one
    models:
      - name: model-a
      - name: model-b
  - name: codex-two
    api-key: key-two
    models:
      - name: model-c
"#;

fn openai_sources(content: &str) -> Vec<ResolvedThinkingAliasSource> {
    resolved_oauth_alias_sources(
        content,
        &[],
        &test_agent_models(&["model-a", "model-b", "model-c"]),
        AliasSourceCapability::Base,
    )
    .unwrap()
}

fn codex_sources(content: &str) -> Vec<ResolvedThinkingAliasSource> {
    resolved_oauth_alias_sources(
        content,
        &[],
        &test_agent_models(&["model-a", "model-b", "model-c"]),
        AliasSourceCapability::Base,
    )
    .unwrap()
}

fn source_for_model<'a>(
    sources: &'a [ResolvedThinkingAliasSource],
    model: &str,
) -> &'a ResolvedThinkingAliasSource {
    sources
        .iter()
        .find(|source| source.source.model == model)
        .unwrap_or_else(|| panic!("source {model} not found"))
}

fn value(content: &str) -> serde_json::Value {
    serde_json::to_value(serde_norway::from_str::<serde_norway::Value>(content).unwrap()).unwrap()
}

#[test]
fn same_alias_across_distinct_openai_providers_is_allowed() {
    let sources = openai_sources(OPENAI_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        OPENAI_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "",
        false,
    )
    .unwrap();
    let second = add_model_alias_to_yaml(&first, source_for_model(&sources, "model-c"), "best", "", false)
        .expect("the same alias must be allowed on another provider");
    let entries = thinking_aliases_from_yaml(&second).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.alias == "best"));
    assert_eq!(entries.iter().find(|e| e.source_model == "model-a").unwrap().provider_index, Some(0));
    assert_eq!(entries.iter().find(|e| e.source_model == "model-c").unwrap().provider_index, Some(1));
}

#[test]
fn same_alias_within_one_openai_provider_pools_the_upstreams() {
    let sources = openai_sources(OPENAI_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        OPENAI_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "",
        false,
    )
    .unwrap();
    let second = add_model_alias_to_yaml(&first, source_for_model(&sources, "model-b"), "best", "", false)
        .expect("openai-compatibility providers may round-robin same-alias upstream models");
    let after = value(&second);
    let models = after["openai-compatibility"][0]["models"].as_array().unwrap();
    assert_eq!(models.len(), 4);
    assert_eq!(models[2]["name"], "model-a");
    assert_eq!(models[2]["alias"], "best");
    assert_eq!(models[3]["name"], "model-b");
    assert_eq!(models[3]["alias"], "best");
}

#[test]
fn same_alias_within_one_codex_provider_is_rejected() {
    let sources = codex_sources(CODEX_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        CODEX_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "",
        false,
    )
    .unwrap();
    let result =
        add_model_alias_to_yaml(&first, source_for_model(&sources, "model-b"), "best", "", false);
    assert!(result.is_err(), "codex-api-key aliases resolve first-entry-wins inside one provider");
}

#[test]
fn same_alias_across_distinct_codex_providers_is_allowed() {
    let sources = codex_sources(CODEX_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        CODEX_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "",
        false,
    )
    .unwrap();
    let second = add_model_alias_to_yaml(&first, source_for_model(&sources, "model-c"), "best", "", false)
        .expect("the same alias must be allowed on another provider");
    let entries = thinking_aliases_from_yaml(&second).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn targeted_delete_keeps_sibling_source_and_shared_payload() {
    let sources = openai_sources(OPENAI_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        OPENAI_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "high",
        false,
    )
    .unwrap();
    let both =
        add_model_alias_to_yaml(&first, source_for_model(&sources, "model-c"), "best", "high", false)
            .unwrap();

    let deleted = remove_thinking_alias_from_yaml_for_channel(
        &both,
        "best",
        None,
        Some(ConfigModelKey {
            section: "openai-compatibility",
            provider_index: 0,
            model_index:  2,
        }),
    )
    .unwrap();
    let entries = thinking_aliases_from_yaml(&deleted).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_model, "model-c");
    let document = value(&deleted);
    let root = document.as_object().unwrap();
    let overrides = root["payload"]["override"].as_array().unwrap();
    assert_eq!(overrides.len(), 1, "sibling source must keep the effort rule");
    assert_eq!(overrides[0]["models"][0]["name"], "best");

    let deleted_last = remove_thinking_alias_from_yaml_for_channel(
        &deleted,
        "best",
        None,
        Some(ConfigModelKey {
            section: "openai-compatibility",
            provider_index: 1,
            model_index: 1,
        }),
    )
    .unwrap();
    assert!(thinking_aliases_from_yaml(&deleted_last).unwrap().is_empty());
    assert!(!deleted_last.contains("payload:"), "removing the last source clears the override");
}

#[test]
fn renaming_one_source_of_shared_alias_keeps_the_other() {
    let sources = openai_sources(OPENAI_TWO_PROVIDERS);
    let first = add_model_alias_to_yaml(
        OPENAI_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "shared",
        "low",
        false,
    )
    .unwrap();
    let both =
        add_model_alias_to_yaml(&first, source_for_model(&sources, "model-c"), "shared", "", false)
            .unwrap();

    let key = ConfigModelKey {
        section: "openai-compatibility",
        provider_index: 0,
        model_index:  2,
    };
    let edit_source = resolve_model_alias_edit_source(&both, "shared", Some(key), &[]).unwrap();
    assert_eq!(edit_source.source.model, "model-a");
    let renamed = edit_model_alias_in_yaml(
        &both,
        "shared",
        Some(key),
        &edit_source,
        "renamed",
        "high",
        false,
    )
    .unwrap();

    let entries = thinking_aliases_from_yaml(&renamed).unwrap();
    assert_eq!(entries.len(), 2);
    let renamed_entry = entries.iter().find(|entry| entry.alias == "renamed").unwrap();
    let shared_entry = entries.iter().find(|entry| entry.alias == "shared").unwrap();
    assert_eq!(renamed_entry.source_model, "model-a");
    assert_eq!(renamed_entry.effort.as_deref(), Some("high"));
    assert_eq!(shared_entry.source_model, "model-c");
    assert_eq!(shared_entry.effort.as_deref(), Some("low"));

    let document = value(&renamed);
    let root = document.as_object().unwrap();
    let overrides = root["payload"]["override"].as_array().unwrap();
    let names = |rule: &serde_json::Value| rule["models"][0]["name"].as_str().unwrap().to_string();
    let efforts: Vec<(String, &str)> = overrides
        .iter()
        .map(|rule| (names(rule), rule["params"]["reasoning_effort"].as_str().unwrap()))
        .collect();
    assert!(efforts.contains(&("renamed".to_string(), "high")));
    assert!(efforts.contains(&("shared".to_string(), "low")));
}

#[test]
fn moving_an_entry_to_another_openai_provider_keeps_the_alias() {
    let sources = openai_sources(OPENAI_TWO_PROVIDERS);
    let created = add_model_alias_to_yaml(
        OPENAI_TWO_PROVIDERS,
        source_for_model(&sources, "model-a"),
        "best",
        "",
        false,
    )
    .unwrap();

    let key = ConfigModelKey {
        section: "openai-compatibility",
        provider_index: 0,
        model_index:  2,
    };
    let edit_source = resolve_model_alias_edit_source(&created, "best", Some(key), &[]).unwrap();
    let moved = edit_model_alias_in_yaml(
        &created,
        "best",
        Some(key),
        source_for_model(&openai_sources(&created), "model-c"),
        "best",
        "",
        false,
    )
    .unwrap();
    let entries = thinking_aliases_from_yaml(&moved).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_model, "model-c");
    assert_eq!(entries[0].provider_index, Some(1));
    assert_eq!(edit_source.source.model, "model-a");
}
