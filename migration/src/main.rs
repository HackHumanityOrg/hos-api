use sea_orm_migration::prelude::*;

#[async_std::main]
async fn main() {
    normalize_database_url_for_cli();
    cli::run_cli(migration::Migrator).await;
}

fn normalize_database_url_for_cli() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        return;
    };

    let Some((base, query)) = database_url.split_once('?') else {
        return;
    };

    let uses_system_rootcert = query
        .split('&')
        .any(|entry| entry.eq_ignore_ascii_case("sslrootcert=system"));

    if !uses_system_rootcert {
        return;
    }

    let mut rewritten: Vec<String> = query
        .split('&')
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslrootcert="))
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslmode="))
        .map(ToString::to_string)
        .collect();
    rewritten.push("sslmode=require".to_string());

    std::env::set_var("DATABASE_URL", format!("{base}?{}", rewritten.join("&")));
}
