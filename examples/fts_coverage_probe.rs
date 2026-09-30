//! Read-only probe (aegis-4h7zw8): FTS/vector index coverage of a bobbin Lance
//! store. Opens the table and reads index metadata only: no compaction, no
//! writes. Usage: fts_coverage_probe <path-to-.bobbin/vectors>
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path to the vectors dir");
    let db = lancedb::connect(&path).execute().await?;
    let names = db.table_names().execute().await?;
    println!("tables: {names:?}");
    for name in names {
        let table = db.open_table(&name).execute().await?;
        let rows = table.count_rows(None).await?;
        println!("{name}: rows={rows} version={}", table.version().await?);
        for index in table.list_indices().await? {
            let stats = table.index_stats(&index.name).await?;
            match stats {
                Some(s) => println!(
                    "  index {} type={:?} columns={:?} indexed={} unindexed={}",
                    index.name,
                    index.index_type,
                    index.columns,
                    s.num_indexed_rows,
                    s.num_unindexed_rows
                ),
                None => println!("  index {} (no stats)", index.name),
            }
        }
    }
    Ok(())
}
