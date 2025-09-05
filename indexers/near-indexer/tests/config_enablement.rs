use near_indexer::modules;
use near_indexer::ModuleRegistry;

fn base_indexer_config() -> common::config::IndexerConfig {
    common::config::IndexerConfig {
        database: common::config::DatabaseConfig {
            url: String::new(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            acquire_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        },
        logging: common::config::LoggingConfig {
            level: "debug".into(),
            format: None,
        },
        telemetry: common::config::TelemetryConfig {
            enabled: false,
            service_version: "test".into(),
            environment: "test".into(),
            enable_metrics: false,
            enable_traces: false,
            enable_logs: false,
            batch_size: 0,
            export_timeout_sec: 0,
            export_interval_sec: 0,
            resource_attributes: Default::default(),
        },
        discourse: None,
        near: Some(common::config::NearLakeConfig::default()),
    }
}

#[test]
fn registry_enables_modules_by_config() {
    // Default NearLakeConfig::default enables chain="mainnet" and no modules -> accounts default true, tokens default false
    let cfg = common::config::AppConfig::Indexer(base_indexer_config());
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &cfg);

    let enabled: Vec<&str> = registry.enabled_modules().map(|m| m.name()).collect();
    assert!(enabled.contains(&"transactions"));
    assert!(enabled.contains(&"accounts"));
    // tokens disabled by default when not specified
    assert!(!enabled.contains(&"tokens"));
}

#[test]
fn tokens_can_be_enabled_via_config() {
    let mut cfg = base_indexer_config();
    cfg.near = Some(common::config::NearLakeConfig {
        chain: "mainnet".into(),
        start_height: 0,
        concurrency: Some(2),
        region: None,
        modules: Some(common::config::NearModulesConfig {
            accounts: Some(common::config::AccountsModuleConfig {
                enabled: true,
                track_implicit: true,
                track_eth_implicit: true,
            }),
            tokens: Some(common::config::TokensModuleConfig {
                enabled: true,
                track_nep141: true,
                track_nep171: true,
                track_nep245: true,
                contracts: vec![],
            }),
            validators: None,
            network: None,
        }),
    });
    let app = common::config::AppConfig::Indexer(cfg);
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &app);

    let enabled: Vec<&str> = registry.enabled_modules().map(|m| m.name()).collect();
    assert!(enabled.contains(&"tokens"));
}

#[test]
fn validators_can_be_enabled_via_config() {
    let mut cfg = base_indexer_config();
    cfg.near = Some(common::config::NearLakeConfig {
        chain: "mainnet".into(),
        start_height: 0,
        concurrency: Some(2),
        region: None,
        modules: Some(common::config::NearModulesConfig {
            accounts: Some(common::config::AccountsModuleConfig {
                enabled: true,
                track_implicit: true,
                track_eth_implicit: true,
            }),
            tokens: None,
            validators: Some(common::config::ValidatorsModuleConfig { enabled: true }),
            network: None,
        }),
    });
    let app = common::config::AppConfig::Indexer(cfg);
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &app);

    let enabled: Vec<&str> = registry.enabled_modules().map(|m| m.name()).collect();
    assert!(enabled.contains(&"validators"));
}
