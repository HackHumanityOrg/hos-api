use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("entity generation failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let database_url = env::var("DATABASE_URL").map_err(|_| {
        "DATABASE_URL is not set. Run via direnv/doppler or export DATABASE_URL first.".to_string()
    })?;

    let normalized_url = normalize_database_url(&database_url);

    generate_schema_entities(
        &normalized_url,
        "discourse",
        "shared/entities/src/discourse",
    )?;
    generate_schema_entities(&normalized_url, "near", "shared/entities/src/near")?;

    Ok(())
}

fn normalize_database_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return format!("{url}?sslmode=require");
    };

    let uses_system_rootcert = query
        .split('&')
        .any(|entry| entry.eq_ignore_ascii_case("sslrootcert=system"));

    if !uses_system_rootcert {
        return url.to_string();
    }

    let mut rewritten: Vec<String> = query
        .split('&')
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslrootcert="))
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslmode="))
        .map(ToString::to_string)
        .collect();

    rewritten.push("sslmode=require".to_string());

    format!("{base}?{}", rewritten.join("&"))
}

fn generate_schema_entities(
    database_url: &str,
    schema: &str,
    output_dir: &str,
) -> Result<(), String> {
    let status = Command::new("sea-orm-cli")
        .arg("generate")
        .arg("entity")
        .arg("--database-url")
        .arg(database_url)
        .arg("--database-schema")
        .arg(schema)
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--with-serde")
        .arg("both")
        .status()
        .map_err(|err| {
            format!(
                "failed to run sea-orm-cli; install it with `cargo install sea-orm-cli --version 1.1.15 --locked` ({err})"
            )
        })?;

    if !status.success() {
        return Err(format!(
            "sea-orm-cli generation failed for schema '{schema}'"
        ));
    }

    Ok(())
}
