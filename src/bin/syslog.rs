#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hive_mcp::entry().await
}
