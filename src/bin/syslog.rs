#[path = "../main.rs"]
mod hive_main;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hive_main::entry().await
}
