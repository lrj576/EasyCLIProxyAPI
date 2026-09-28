use super::support::*;
use super::*;

/// End-to-end exercise of the GUI alias pipeline against a real kernel.
///
/// Start a kernel with the multi-source fixture under /private/tmp/easycli-test
/// before running: cli-proxy-api --config /private/tmp/easycli-test/config.yaml
/// When the kernel is not reachable the test skips itself so the default suite
/// stays hermetic.
const E2E_PORT: u16 = 18317;
const E2E_MANAGEMENT_SECRET: &str = "sk-test-management";

fn e2e_gui_config() -> GuiConfigFile {
    let mut config = GuiConfigFile::default();
    config.host = "127.0.0.1".to_string();
    config.port = E2E_PORT;
    config.management_secret_key = E2E_MANAGEMENT_SECRET.to_string();
    config.api_keys = vec![GuiApiKeyEntry {
        key: "sk-test-client".to_string(),
        remark: String::new(),
    }];
    config
}

fn kernel_available(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
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

async fn cleanup_shared_alias(config: &GuiConfigFile) {
    let content = match fetch_management_config_yaml(config).await {
        Ok(content) => content,
        Err(_) => return,
    };
    let Ok(document) = serde_norway::from_str::<serde_norway::Value>(&content) else {
        return;
    };
    let Some(root) = document.as_mapping() else {
        return;
    };
    if count_configured_alias_occurrences(root, "shared-e2e") == 0 {
        return;
    }
    if let Ok(removed) = remove_thinking_alias_from_yaml_for_channel(&content, "shared-e2e", None, None) {
        let _ = put_management_alias_config_changes(config, &content, &removed).await;
    }
}

#[tokio::test]
async fn alias_pipeline_round_trip_against_real_kernel() {
    if !kernel_available(E2E_PORT) {
        eprintln!("skipping alias e2e: kernel on {E2E_PORT} is not running");
        return;
    }
    let config = e2e_gui_config();
    cleanup_shared_alias(&config).await;

    let content = fetch_management_config_yaml(&config).await.unwrap();
    // Mirrors the real create flow: availability comes from the live kernel.
    let available = fetch_agent_models(config.port, effective_agent_api_key(&config)).await.unwrap();
    let sources = resolved_oauth_alias_sources(
        &content,
        &[],
        &available,
        AliasSourceCapability::Base,
    )
    .unwrap();

    // Create a shared alias on the first provider with a reasoning effort.
    let first = add_model_alias_to_yaml(
        &content,
        source_for_model(&sources, "model-a"),
        "shared-e2e",
        "high",
        false,
    )
    .unwrap();
    put_management_alias_config_changes(&config, &content, &first)
        .await
        .unwrap();

    // Re-read through the GUI pipeline and add a second source for the alias.
    let latest = fetch_management_config_yaml(&config).await.unwrap();
    let latest_available =
        fetch_agent_models(config.port, effective_agent_api_key(&config)).await.unwrap();
    let latest_sources = resolved_oauth_alias_sources(
        &latest,
        &[],
        &latest_available,
        AliasSourceCapability::Base,
    )
    .unwrap();
    let second = add_model_alias_to_yaml(
        &latest,
        source_for_model(&latest_sources, "model-c"),
        "shared-e2e",
        "",
        false,
    )
    .expect("the same alias must be allowed on another provider");
    put_management_alias_config_changes(&config, &latest, &second)
        .await
        .unwrap();

    // The kernel hot-reloads the configuration and must expose one merged model.
    let body = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{E2E_PORT}/v1/models"))
        .header("Authorization", format!("Bearer {}", effective_agent_api_key(&config)))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("shared-e2e"), "model list must contain the alias: {body}");

    let reloaded = fetch_management_config_yaml(&config).await.unwrap();
    let entries = thinking_aliases_from_yaml(&reloaded).unwrap();
    let alias_entries: Vec<_> = entries.into_iter().filter(|e| e.alias == "shared-e2e").collect();
    assert_eq!(alias_entries.len(), 2, "both sources must be listed: {alias_entries:?}");
    assert!(alias_entries.iter().all(|e| e.effort.as_deref() == Some("high")));

    // Clean up through the same transactional path.
    cleanup_shared_alias(&config).await;
}
