//! SQLite WASM Translator for 9P.e
//!
//! This translator exposes SQLite databases through synthetic files in the 9P.e filesystem.
//! Users can write SQL queries to files and read results, creating a filesystem-based database interface.

use wasm_bindgen::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use web_sys::console;

// External SQLite JS bindings (sql.js)
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = SQL)]
    type Database;

    #[wasm_bindgen(constructor, js_namespace = SQL)]
    fn new(data: Option<&[u8]>) -> Database;

    #[wasm_bindgen(method, js_namespace = SQL)]
    fn exec(this: &Database, sql: &str) -> JsValue;

    #[wasm_bindgen(method, js_namespace = SQL)]
    fn export(this: &Database) -> js_sys::Uint8Array;

    #[wasm_bindgen(method, js_namespace = SQL)]
    fn close(this: &Database);
}

/// Query result structure
#[derive(Serialize, Deserialize, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub values: Vec<Vec<serde_json::Value>>,
    pub error: Option<String>,
}

/// Database manager for the SQLite translator
#[wasm_bindgen]
pub struct SqliteTranslator {
    databases: HashMap<String, Database>,
    current_db: Option<String>,
    last_query_result: Option<QueryResult>,
}

#[wasm_bindgen]
impl SqliteTranslator {
    /// Create a new SQLite translator instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> SqliteTranslator {
        console::log_1(&"🗄️ SQLite WASM Translator initialized".into());

        Self {
            databases: HashMap::new(),
            current_db: None,
            last_query_result: None,
        }
    }

    /// Create a new database
    #[wasm_bindgen]
    pub fn create_database(&mut self, name: &str) -> bool {
        console::log_1(&format!("Creating database: {}", name).into());

        let db = Database::new(None);
        self.databases.insert(name.to_string(), db);

        if self.current_db.is_none() {
            self.current_db = Some(name.to_string());
        }

        true
    }

    /// Switch to a different database
    #[wasm_bindgen]
    pub fn use_database(&mut self, name: &str) -> bool {
        if self.databases.contains_key(name) {
            self.current_db = Some(name.to_string());
            console::log_1(&format!("Switched to database: {}", name).into());
            true
        } else {
            console::log_1(&format!("Database not found: {}", name).into());
            false
        }
    }

    /// Execute SQL query on current database
    #[wasm_bindgen]
    pub fn execute_sql(&mut self, sql: &str) -> String {
        let db_name = match &self.current_db {
            Some(name) => name,
            None => {
                let error = QueryResult {
                    columns: vec![],
                    values: vec![],
                    error: Some("No database selected".to_string()),
                };
                self.last_query_result = Some(error.clone());
                return serde_json::to_string(&error).unwrap();
            }
        };

        let db = match self.databases.get(db_name) {
            Some(database) => database,
            None => {
                let error = QueryResult {
                    columns: vec![],
                    values: vec![],
                    error: Some(format!("Database '{}' not found", db_name)),
                };
                self.last_query_result = Some(error.clone());
                return serde_json::to_string(&error).unwrap();
            }
        };

        console::log_1(&format!("Executing SQL: {}", sql).into());

        // Execute the SQL
        let result = db.exec(sql);

        // Parse the result (this is a simplified version)
        let query_result = if result.is_undefined() || result.is_null() {
            QueryResult {
                columns: vec![],
                values: vec![],
                error: None,
            }
        } else {
            // In a real implementation, we'd parse the SQL.js result structure
            // For now, return a simple success message
            QueryResult {
                columns: vec!["status".to_string()],
                values: vec![vec![serde_json::Value::String("Query executed successfully".to_string())]],
                error: None,
            }
        };

        self.last_query_result = Some(query_result.clone());
        serde_json::to_string(&query_result).unwrap()
    }

    /// Get the last query result
    #[wasm_bindgen]
    pub fn get_last_result(&self) -> String {
        match &self.last_query_result {
            Some(result) => serde_json::to_string(result).unwrap(),
            None => {
                let empty = QueryResult {
                    columns: vec![],
                    values: vec![],
                    error: Some("No queries executed yet".to_string()),
                };
                serde_json::to_string(&empty).unwrap()
            }
        }
    }

    /// List all databases
    #[wasm_bindgen]
    pub fn list_databases(&self) -> String {
        let db_names: Vec<&String> = self.databases.keys().collect();
        serde_json::to_string(&db_names).unwrap()
    }

    /// Get current database name
    #[wasm_bindgen]
    pub fn current_database(&self) -> String {
        self.current_db.clone().unwrap_or_else(|| "none".to_string())
    }

    /// Get database schema (simplified)
    #[wasm_bindgen]
    pub fn get_schema(&mut self) -> String {
        let schema_query = "SELECT name, sql FROM sqlite_master WHERE type='table';";
        self.execute_sql(schema_query)
    }

    /// Drop a database
    #[wasm_bindgen]
    pub fn drop_database(&mut self, name: &str) -> bool {
        if let Some(db) = self.databases.remove(name) {
            db.close();
            console::log_1(&format!("Dropped database: {}", name).into());

            // If we dropped the current database, clear current_db
            if self.current_db.as_ref() == Some(&name.to_string()) {
                self.current_db = None;
            }

            true
        } else {
            false
        }
    }
}

/// Translator interface for 9P.e integration
#[wasm_bindgen]
pub struct TranslatorInterface {
    sqlite: SqliteTranslator,
}

#[wasm_bindgen]
impl TranslatorInterface {
    /// Initialize the translator
    #[wasm_bindgen(constructor)]
    pub fn new() -> TranslatorInterface {
        console::log_1(&"🔧 Initializing SQLite Translator Interface".into());

        let mut sqlite = SqliteTranslator::new();
        // Create a default database
        sqlite.create_database("default");

        Self { sqlite }
    }

    /// Handle synthetic file read operations
    #[wasm_bindgen]
    pub fn read_file(&mut self, path: &str) -> String {
        console::log_1(&format!("Reading synthetic file: {}", path).into());

        match path {
            "result.json" => self.sqlite.get_last_result(),
            "schema.sql" => self.sqlite.get_schema(),
            "databases.json" => self.sqlite.list_databases(),
            "current_db.txt" => self.sqlite.current_database(),
            _ if path.starts_with("databases/") => {
                let db_name = &path[10..]; // Remove "databases/" prefix
                if self.sqlite.databases.contains_key(db_name) {
                    format!("Database: {}\nStatus: Active", db_name)
                } else {
                    "Database not found".to_string()
                }
            }
            _ => format!("Unknown file: {}", path),
        }
    }

    /// Handle synthetic file write operations
    #[wasm_bindgen]
    pub fn write_file(&mut self, path: &str, content: &str) -> String {
        console::log_1(&format!("Writing to synthetic file: {} ({})", path, content.len()).into());

        match path {
            "query.sql" => {
                let _result = self.sqlite.execute_sql(content);
                "Query executed. Check result.json for output.".to_string()
            }
            "create_db.txt" => {
                let db_name = content.trim();
                if self.sqlite.create_database(db_name) {
                    format!("Database '{}' created successfully", db_name)
                } else {
                    format!("Failed to create database '{}'", db_name)
                }
            }
            "use_db.txt" => {
                let db_name = content.trim();
                if self.sqlite.use_database(db_name) {
                    format!("Switched to database '{}'", db_name)
                } else {
                    format!("Database '{}' not found", db_name)
                }
            }
            "drop_db.txt" => {
                let db_name = content.trim();
                if self.sqlite.drop_database(db_name) {
                    format!("Database '{}' dropped successfully", db_name)
                } else {
                    format!("Failed to drop database '{}'", db_name)
                }
            }
            _ => format!("Cannot write to file: {}", path),
        }
    }

    /// List available synthetic files
    #[wasm_bindgen]
    pub fn list_files(&self) -> String {
        let files = vec![
            "query.sql",      // Write SQL queries here
            "result.json",    // Read query results
            "schema.sql",     // Read current database schema
            "databases.json", // List all databases
            "current_db.txt", // Current database name
            "create_db.txt",  // Write database name to create
            "use_db.txt",     // Write database name to switch to
            "drop_db.txt",    // Write database name to drop
        ];
        serde_json::to_string(&files).unwrap()
    }
}

/// Initialize the WASM module
#[wasm_bindgen(start)]
pub fn main() {
    console::log_1(&"🚀 SQLite WASM Translator loaded!".into());
}

// For testing without WASM
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    use super::*;

    pub use crate::TranslatorInterface;

    pub fn test_translator() {
        let mut translator = TranslatorInterface::new();

        // Test creating a database
        let result = translator.write_file("create_db.txt", "test_db");
        println!("Create DB: {}", result);

        // Test switching database
        let result = translator.write_file("use_db.txt", "test_db");
        println!("Use DB: {}", result);

        // Test SQL execution
        let result = translator.write_file("query.sql", "CREATE TABLE users (id INTEGER, name TEXT);");
        println!("Create Table: {}", result);

        // Test reading result
        let result = translator.read_file("result.json");
        println!("Result: {}", result);

        // Test listing files
        let files = translator.list_files();
        println!("Available files: {}", files);
    }
}

// Include native test module
#[cfg(not(target_arch = "wasm32"))]
pub mod native_test;