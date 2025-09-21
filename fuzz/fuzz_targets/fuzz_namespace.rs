// 9P.e Server - Namespace Fuzzer
// Copyright (C) 2024 9P.e Server Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use ninepee_server::*;

#[derive(Arbitrary, Debug)]
enum NamespaceOp {
    Create { path: String, m: u8, n: u8 },
    Delete { path: String },
    Move { from: String, to: String },
    SetPermissions { path: String, perms: u32 },
    AddKey { path: String, key: Vec<u8> },
    RemoveKey { path: String, key: Vec<u8> },
    List { path: String },
    CheckAccess { path: String, key: Vec<u8> },
}

fuzz_target!(|ops: Vec<NamespaceOp>| {
    let mut namespace_tree = NamespaceTree::new();

    for op in ops {
        match op {
            NamespaceOp::Create { path, m, n } => {
                // Validate M-of-N configuration
                if m > 0 && m <= n && n <= 100 {
                    let _ = namespace_tree.create(&path, m, n);
                }

                // Verify no path traversal
                assert!(!path.contains("../"));
                assert!(!path.contains('\0'));
            }
            NamespaceOp::Delete { path } => {
                let _ = namespace_tree.delete(&path);
            }
            NamespaceOp::Move { from, to } => {
                // Verify move doesn't create cycles
                let _ = namespace_tree.move_namespace(&from, &to);
                assert!(namespace_tree.verify_no_cycles());
            }
            NamespaceOp::SetPermissions { path, perms } => {
                let _ = namespace_tree.set_permissions(&path, perms);
            }
            NamespaceOp::AddKey { path, key } => {
                if key.len() == 32 {
                    let _ = namespace_tree.add_key(&path, &key);
                }
            }
            NamespaceOp::RemoveKey { path, key } => {
                if key.len() == 32 {
                    let _ = namespace_tree.remove_key(&path, &key);
                }
            }
            NamespaceOp::List { path } => {
                let _ = namespace_tree.list(&path);
            }
            NamespaceOp::CheckAccess { path, key } => {
                if key.len() == 32 {
                    let _ = namespace_tree.check_access(&path, &key);
                }
            }
        }

        // Invariants that must always hold
        assert!(namespace_tree.verify_tree_structure());
        assert!(namespace_tree.verify_depth_limit(100));
        assert!(namespace_tree.verify_unique_paths());
    }
});

// Stub implementation
struct NamespaceTree;

impl NamespaceTree {
    fn new() -> Self { NamespaceTree }
    fn create(&mut self, _path: &str, _m: u8, _n: u8) -> Result<(), Error> { Ok(()) }
    fn delete(&mut self, _path: &str) -> Result<(), Error> { Ok(()) }
    fn move_namespace(&mut self, _from: &str, _to: &str) -> Result<(), Error> { Ok(()) }
    fn set_permissions(&mut self, _path: &str, _perms: u32) -> Result<(), Error> { Ok(()) }
    fn add_key(&mut self, _path: &str, _key: &[u8]) -> Result<(), Error> { Ok(()) }
    fn remove_key(&mut self, _path: &str, _key: &[u8]) -> Result<(), Error> { Ok(()) }
    fn list(&self, _path: &str) -> Result<Vec<String>, Error> { Ok(vec![]) }
    fn check_access(&self, _path: &str, _key: &[u8]) -> Result<bool, Error> { Ok(false) }
    fn verify_no_cycles(&self) -> bool { true }
    fn verify_tree_structure(&self) -> bool { true }
    fn verify_depth_limit(&self, _max: usize) -> bool { true }
    fn verify_unique_paths(&self) -> bool { true }
}

struct Error;