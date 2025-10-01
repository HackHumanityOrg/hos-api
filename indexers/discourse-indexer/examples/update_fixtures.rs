use anyhow::Result;
use discourse_indexer::discourse_api::DiscourseClient;
use std::path::PathBuf;
use tokio::fs;

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = "https://meta.discourse.org";
    let client = DiscourseClient::new(base_url.to_string());

    // Base path for fixtures
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("meta");

    println!("Updating fixtures in: {:?}", fixtures_dir);
    println!("Fetching from: {}", base_url);
    println!();

    // Fetch categories (pages 0-2)
    for page in 0..=2 {
        let path = format!("/categories.json?include_subcategories=true&page={}", page);
        let filename = format!("categories_page_{}.json", page);
        fetch_and_save(&client, &path, &fixtures_dir, &filename).await?;
    }

    // Fetch directory items (pages 0-10)
    for page in 0..=10 {
        let path = format!(
            "/directory_items.json?period=all&order=likes_received&page={}",
            page
        );
        let filename = format!("directory_items_page_{}.json", page);
        fetch_and_save(&client, &path, &fixtures_dir, &filename).await?;
    }

    // Fetch latest activity
    let path = "/latest.json?page=0";
    let filename = "latest_activity_page_0.json";
    fetch_and_save(&client, path, &fixtures_dir, filename).await?;

    // Fetch topic 69776 posts
    let path = "/t/69776.json?page=0&include_raw=true";
    let filename = "topic_69776_posts_page_0.json";
    fetch_and_save(&client, path, &fixtures_dir, filename).await?;

    // Fetch post 326680 likes
    let path = "/post_action_users.json?id=326680&post_action_type_id=2";
    let filename = "post_326680_likes.json";
    fetch_and_save(&client, path, &fixtures_dir, filename).await?;

    // Fetch post 326680 revision 2
    let path = "/posts/326680/revisions/2.json";
    let filename = "post_326680_revision_2.json";
    fetch_and_save(&client, path, &fixtures_dir, filename).await?;

    println!();
    println!("✓ All fixtures updated successfully!");

    Ok(())
}

async fn fetch_and_save(
    client: &DiscourseClient,
    path: &str,
    fixtures_dir: &PathBuf,
    filename: &str,
) -> Result<()> {
    print!("Fetching {} ... ", filename);
    std::io::Write::flush(&mut std::io::stdout())?;

    let json: serde_json::Value = client.get_json(path).await?;
    let pretty_json = serde_json::to_string_pretty(&json)?;

    let file_path = fixtures_dir.join(filename);
    fs::write(&file_path, pretty_json).await?;

    println!("✓ ({} bytes)", fs::metadata(&file_path).await?.len());

    Ok(())
}
