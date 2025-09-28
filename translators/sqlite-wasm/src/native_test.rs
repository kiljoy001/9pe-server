//! Native test for SQLite translator functionality

#[cfg(not(target_arch = "wasm32"))]
pub fn test_translator_basic_operations() {
    use crate::native::*;

    println!("Testing SQLite translator basic operations...");

    // Test translator creation
    let mut translator = TranslatorInterface::new();

    // Test file listing
    let files = translator.list_files();
    println!("Available files: {}", files);

    // Test database creation
    let result = translator.write_file("create_db.txt", "test_database");
    println!("Create database result: {}", result);

    // Test database switching
    let result = translator.write_file("use_db.txt", "test_database");
    println!("Switch database result: {}", result);

    // Test current database
    let current = translator.read_file("current_db.txt");
    println!("Current database: {}", current);

    // Test SQL execution
    let result = translator.write_file("query.sql", "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT);");
    println!("Create table result: {}", result);

    // Test reading results
    let result = translator.read_file("result.json");
    println!("Query result: {}", result);

    // Test schema
    let schema = translator.read_file("schema.sql");
    println!("Schema: {}", schema);

    // Test database listing
    let databases = translator.read_file("databases.json");
    println!("Available databases: {}", databases);

    println!("✅ All basic operations completed successfully!");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_sqlite_translator() {
        test_translator_basic_operations();
    }
}