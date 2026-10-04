use std::path::Path;

use libsql::Connection;

use crate::util::{Res, uuid};

fn body(sql: &str) -> String {
    sql.split_once('(').map_or("", |(_, b)| b).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tables(schema: &str) -> impl Iterator<Item = (&str, &str)> {
    schema.split(';').filter_map(|s| {
        let rest = s.trim().strip_prefix("CREATE TABLE IF NOT EXISTS ")?;
        Some((rest.split_whitespace().next()?, s.trim()))
    })
}

async fn strings(db: &Connection, sql: &str, p: impl libsql::params::IntoParams) -> Res<Vec<String>> {
    let mut rows = db.query(sql, p).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(row.get::<String>(0)?);
    }
    Ok(out)
}

async fn columns(db: &Connection, table: &str) -> Res<Vec<String>> {
    strings(db, "SELECT name FROM pragma_table_info(?1)", [table]).await
}

async fn rebuild(db: &Connection, name: &str, create: &str) -> Res<()> {
    let tmp = format!("{name}__migrate");
    let old = columns(db, name).await?;
    db.execute(&format!("DROP TABLE IF EXISTS {tmp}"), ()).await?;
    db.execute(&format!("CREATE TABLE {tmp} ({}", create.split_once('(').map_or("", |(_, b)| b)), ()).await?;
    let new = columns(db, &tmp).await?;
    let (kept, dropped): (Vec<_>, Vec<_>) = old.into_iter().partition(|c| new.contains(c));
    if !dropped.is_empty() {
        println!("migrate: {name} drops columns {dropped:?}");
    }
    let cols = kept.join(", ");
    db.execute(&format!("INSERT INTO {tmp} ({cols}) SELECT {cols} FROM {name}"), ()).await?;
    db.execute(&format!("DROP TABLE {name}"), ()).await?;
    db.execute(&format!("ALTER TABLE {tmp} RENAME TO {name}"), ()).await?;
    println!("migrate: {name} updated");
    Ok(())
}

async fn outdated<'a>(db: &Connection, schema: &'a str) -> Res<Vec<(&'a str, &'a str)>> {
    let mut changed = Vec::new();
    for (name, create) in tables(schema) {
        let current = strings(db, "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [name]).await?;
        if current.first().is_some_and(|c| body(c) != body(create)) {
            changed.push((name, create));
        }
    }
    Ok(changed)
}

async fn legacy_ids(db: &Connection, table: &str) -> Res<Vec<String>> {
    let sql = "SELECT name FROM pragma_table_info(?1) WHERE name = 'id' AND upper(type) = 'TEXT'";
    if strings(db, sql, [table]).await?.is_empty() {
        return Ok(Vec::new());
    }
    strings(db, &format!("SELECT CAST(id AS TEXT) FROM {table} WHERE length(id) != 36"), ()).await
}

async fn referencing(db: &Connection, names: &[&str], table: &str) -> Res<Vec<(String, String)>> {
    let mut refs = Vec::new();
    for child in names {
        let sql = "SELECT \"from\" FROM pragma_foreign_key_list(?1) WHERE \"table\" = ?2";
        for col in strings(db, sql, [*child, table]).await? {
            refs.push(((*child).to_owned(), col));
        }
    }
    Ok(refs)
}

async fn rename_id(db: &Connection, table: &str, refs: &[(String, String)], old: &str) -> Res<()> {
    let new = uuid();
    db.execute(&format!("UPDATE {table} SET id = ?1 WHERE id = ?2"), [new.as_str(), old]).await?;
    for (child, col) in refs {
        db.execute(&format!("UPDATE {child} SET {col} = ?1 WHERE {col} = ?2"), [new.as_str(), old]).await?;
    }
    Ok(())
}

async fn assign_uuids(db: &Connection, schema: &str) -> Res<usize> {
    let names: Vec<&str> = tables(schema).map(|(n, _)| n).collect();
    let mut count = 0;
    for table in &names {
        let ids = legacy_ids(db, table).await?;
        let refs = if ids.is_empty() { Vec::new() } else { referencing(db, &names, table).await? };
        for old in &ids {
            rename_id(db, table, &refs, old).await?;
        }
        count += ids.len();
    }
    if count > 0 {
        println!("migrate: {count} ids converted to uuid");
    }
    Ok(count)
}

async fn snapshot(db: &Connection, path: Option<&Path>) -> Res<()> {
    let Some(path) = path else { return Ok(()) };
    db.execute("VACUUM INTO ?1", [path.to_string_lossy().into_owned()]).await?;
    println!("migrate: backup written to {}", path.display());
    Ok(())
}

pub async fn migrate(db: &Connection, schema: &str, backup: Option<&Path>) -> Res<usize> {
    db.execute("PRAGMA foreign_keys = OFF", ()).await?;
    let changed = outdated(db, schema).await?;
    snapshot(db, backup.filter(|_| !changed.is_empty())).await?;
    let tx = db.transaction().await?;
    for (name, create) in &changed {
        rebuild(&tx, name, create).await?;
    }
    tx.execute_batch(schema).await?;
    assign_uuids(&tx, schema).await?;
    tx.commit().await?;
    Ok(changed.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, title TEXT NOT NULL, gone TEXT);
        CREATE INDEX IF NOT EXISTS t_title ON t(title);";
    const V2: &str = "CREATE TABLE IF NOT EXISTS t (
          id INTEGER PRIMARY KEY, title TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 7);
        CREATE INDEX IF NOT EXISTS t_title ON t(title);";

    #[tokio::test]
    async fn keeps_rows_across_schema_change() {
        let db = libsql::Builder::new_local(":memory:").build().await.unwrap().connect().unwrap();
        assert_eq!(migrate(&db, V1, None).await.ok(), Some(0));
        db.execute("INSERT INTO t (id, title, gone) VALUES (1, 'a', 'x')", ()).await.unwrap();
        assert_eq!(migrate(&db, V1, None).await.ok(), Some(0));
        assert_eq!(migrate(&db, V2, None).await.ok(), Some(1));
        assert_eq!(migrate(&db, V2, None).await.ok(), Some(0));
        let mut rows = db.query("SELECT title, position FROM t WHERE id = 1", ()).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "a");
        assert_eq!(row.get::<i64>(1).unwrap(), 7);
        assert_eq!(strings(&db, "SELECT name FROM sqlite_master WHERE type = 'index'", ()).await.ok().unwrap(), ["t_title"]);
    }

    const OLD_IDS: &str = "CREATE TABLE IF NOT EXISTS p (id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE IF NOT EXISTS c (id INTEGER PRIMARY KEY, p_id INTEGER);";
    const UUID_IDS: &str = "CREATE TABLE IF NOT EXISTS p (id TEXT PRIMARY KEY, name TEXT);
        CREATE TABLE IF NOT EXISTS c (id TEXT PRIMARY KEY, p_id TEXT REFERENCES p(id));";

    #[tokio::test]
    async fn converts_integer_ids_to_uuids() {
        let db = libsql::Builder::new_local(":memory:").build().await.unwrap().connect().unwrap();
        migrate(&db, OLD_IDS, None).await.unwrap_or_else(|e| panic!("{}", e.1));
        db.execute_batch("INSERT INTO p VALUES (1, 'a'), (2, 'b'); INSERT INTO c VALUES (1, 2), (2, NULL);").await.unwrap();
        migrate(&db, UUID_IDS, None).await.unwrap_or_else(|e| panic!("{}", e.1));
        let sql = "SELECT p.name FROM c JOIN p ON p.id = c.p_id WHERE length(p.id) = 36 AND length(c.id) = 36";
        assert_eq!(strings(&db, sql, ()).await.ok().unwrap(), ["b"]);
        let short = "SELECT id FROM p WHERE length(id) != 36 UNION ALL SELECT id FROM c WHERE length(id) != 36";
        assert!(strings(&db, short, ()).await.ok().unwrap().is_empty());
        assert_eq!(migrate(&db, UUID_IDS, None).await.ok(), Some(0));
    }
}
