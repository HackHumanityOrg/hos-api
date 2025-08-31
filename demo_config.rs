// Demo of the new type-safe configuration system
use common::config::AppConfig;

fn main() -> anyhow::Result<()> {
    // Load API configuration (includes server settings)
    let api_config = AppConfig::load_api()?;
    println!("API Config: {:?}", api_config);
    
    // Type-safe access to server config (only available for API)
    if let Some(server) = api_config.server() {
        println!("API Server: {}:{}", server.host, server.port);
    }
    
    // This would work too (and panic if not API)
    let server = api_config.server_required();
    println!("API Server (required): {}:{}", server.host, server.port);
    
    // Load indexer configuration (no server settings)
    let indexer_config = AppConfig::load_indexer("indexer-1")?;
    println!("Indexer Config: {:?}", indexer_config);
    
    // This returns None for indexers (type-safe!)
    if let Some(server) = indexer_config.server() {
        println!("Indexer Server: {}:{}", server.host, server.port);
    } else {
        println!("✅ Indexer correctly has no server config");
    }
    
    // Both configs have database and logging (shared interface)
    println!("API Database: max_connections = {:?}", api_config.database().max_connections);
    println!("Indexer Database: max_connections = {:?}", indexer_config.database().max_connections);
    
    println!("API Logging: level = {}", api_config.logging().level);
    println!("Indexer Logging: level = {}", indexer_config.logging().level);
    
    Ok(())
}