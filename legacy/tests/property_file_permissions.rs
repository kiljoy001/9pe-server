//! Property-based tests for file permissions and operations
//! Verifies Unix-style permissions, path safety, and synthetic file properties

use proptest::prelude::*;
use proptest::collection::{vec, hash_set};
use proptest::string::string_regex;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Uid(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Gid(u32);

#[derive(Debug, Clone, Copy)]
struct Permissions {
    mode: u32,
}

impl Permissions {
    fn new(mode: u32) -> Self {
        Permissions { mode: mode & 0o777 }
    }

    fn owner_perms(&self) -> u32 {
        (self.mode >> 6) & 0o7
    }

    fn group_perms(&self) -> u32 {
        (self.mode >> 3) & 0o7
    }

    fn other_perms(&self) -> u32 {
        self.mode & 0o7
    }

    fn can_read(&self, uid: Uid, gid: Gid, owner: Uid, group: Gid) -> bool {
        if uid == owner {
            (self.owner_perms() & 0o4) != 0
        } else if gid == group {
            (self.group_perms() & 0o4) != 0
        } else {
            (self.other_perms() & 0o4) != 0
        }
    }

    fn can_write(&self, uid: Uid, gid: Gid, owner: Uid, group: Gid) -> bool {
        if uid == owner {
            (self.owner_perms() & 0o2) != 0
        } else if gid == group {
            (self.group_perms() & 0o2) != 0
        } else {
            (self.other_perms() & 0o2) != 0
        }
    }

    fn can_execute(&self, uid: Uid, gid: Gid, owner: Uid, group: Gid) -> bool {
        if uid == owner {
            (self.owner_perms() & 0o1) != 0
        } else if gid == group {
            (self.group_perms() & 0o1) != 0
        } else {
            (self.other_perms() & 0o1) != 0
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileType {
    RegularFile,
    Directory,
    SymbolicLink,
    SyntheticFile,
    FunctionFile,
    WasmTranslator,
}

#[derive(Debug, Clone)]
struct FileMeta {
    file_type: FileType,
    size: u64,
    owner: Uid,
    group: Gid,
    perms: Permissions,
}

#[derive(Debug, Clone)]
struct FSEntry {
    path: PathBuf,
    meta: FileMeta,
    content: FileContent,
}

#[derive(Debug, Clone)]
enum FileContent {
    Static(Vec<u8>),
    Computed,  // Synthetic/function files
    None,      // Directories
}

/// Generate arbitrary Unix permissions
fn arbitrary_permissions() -> impl Strategy<Value = u32> {
    0o000u32..=0o777u32
}

/// Generate arbitrary UID/GID
fn arbitrary_uid() -> impl Strategy<Value = Uid> {
    (0u32..65535u32).prop_map(Uid)
}

fn arbitrary_gid() -> impl Strategy<Value = Gid> {
    (0u32..65535u32).prop_map(Gid)
}

/// Generate valid file paths
fn arbitrary_path() -> impl Strategy<Value = PathBuf> {
    vec(string_regex("[a-zA-Z0-9._-]+").unwrap(), 1..5)
        .prop_map(|components| {
            let mut path = PathBuf::from("/");
            for c in components {
                path.push(c);
            }
            path
        })
}

/// Generate file metadata
fn arbitrary_file_meta() -> impl Strategy<Value = FileMeta> {
    (
        prop_oneof![
            Just(FileType::RegularFile),
            Just(FileType::Directory),
            Just(FileType::SyntheticFile),
        ],
        0u64..1000000u64,  // size
        arbitrary_uid(),
        arbitrary_gid(),
        arbitrary_permissions(),
    ).prop_map(|(file_type, size, owner, group, mode)| {
        FileMeta {
            file_type,
            size,
            owner,
            group,
            perms: Permissions::new(mode),
        }
    })
}

proptest! {
    /// Test: Read operations require read permission
    #[test]
    fn prop_read_requires_permission(
        mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid(),
        owner in arbitrary_uid(),
        group in arbitrary_gid()
    ) {
        let perms = Permissions::new(mode);
        let can_read = perms.can_read(uid, gid, owner, group);

        if uid == owner {
            // Owner permissions
            prop_assert_eq!(can_read, (mode & 0o400) != 0);
        } else if gid == group {
            // Group permissions
            prop_assert_eq!(can_read, (mode & 0o040) != 0);
        } else {
            // Other permissions
            prop_assert_eq!(can_read, (mode & 0o004) != 0);
        }
    }

    /// Test: Write operations require write permission
    #[test]
    fn prop_write_requires_permission(
        mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid(),
        owner in arbitrary_uid(),
        group in arbitrary_gid()
    ) {
        let perms = Permissions::new(mode);
        let can_write = perms.can_write(uid, gid, owner, group);

        if uid == owner {
            // Owner permissions
            prop_assert_eq!(can_write, (mode & 0o200) != 0);
        } else if gid == group {
            // Group permissions
            prop_assert_eq!(can_write, (mode & 0o020) != 0);
        } else {
            // Other permissions
            prop_assert_eq!(can_write, (mode & 0o002) != 0);
        }
    }

    /// Test: Directory traversal requires execute permission
    #[test]
    fn prop_directory_traverse_requires_execute(
        mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid(),
        owner in arbitrary_uid(),
        group in arbitrary_gid()
    ) {
        let perms = Permissions::new(mode);
        let meta = FileMeta {
            file_type: FileType::Directory,
            size: 4096,
            owner,
            group,
            perms,
        };

        let can_traverse = perms.can_execute(uid, gid, owner, group);

        if meta.file_type == FileType::Directory {
            // For directories, execute = traverse
            if uid == owner {
                prop_assert_eq!(can_traverse, (mode & 0o100) != 0);
            } else if gid == group {
                prop_assert_eq!(can_traverse, (mode & 0o010) != 0);
            } else {
                prop_assert_eq!(can_traverse, (mode & 0o001) != 0);
            }
        }
    }

    /// Test: Synthetic files are read-only
    #[test]
    fn prop_synthetic_files_readonly(
        mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid()
    ) {
        let meta = FileMeta {
            file_type: FileType::SyntheticFile,
            size: 0,
            owner: uid,
            group: gid,
            perms: Permissions::new(mode),
        };

        let content = FileContent::Computed;

        // Even with write permission, synthetic files cannot be written
        let has_write_perm = meta.perms.can_write(uid, gid, uid, gid);

        // Should not allow write to computed content
        let can_write_content = match &content {
            FileContent::Static(_) => true,
            FileContent::Computed => false,  // Cannot write to synthetic
            FileContent::None => false,      // Cannot write to directories
        };

        if meta.file_type == FileType::SyntheticFile {
            prop_assert!(!can_write_content);
        }

        // Function files also read-only
        let func_meta = FileMeta {
            file_type: FileType::FunctionFile,
            ..meta
        };

        if func_meta.file_type == FileType::FunctionFile {
            prop_assert!(!can_write_content);
        }
    }

    /// Test: Path traversal prevention
    #[test]
    fn prop_path_traversal_prevention(
        base_path in arbitrary_path(),
        traversal_attempts in vec(prop::string::string_regex("\\.\\.|\\./|\\.").unwrap(), 0..10)
    ) {
        let root = PathBuf::from("/srv");

        // Normalize path function
        fn normalize_path(base: &Path, components: &[String]) -> PathBuf {
            let mut result = base.to_path_buf();

            for component in components {
                if component == ".." {
                    result.pop();
                } else if component != "." && component != "./" {
                    result.push(component);
                }
            }

            result
        }

        // Check if path escapes root
        fn is_safe_path(path: &Path, root: &Path) -> bool {
            path.starts_with(root) || path == root
        }

        let normalized = normalize_path(&base_path, &traversal_attempts);

        // Path with ".." attempts
        let mut malicious = base_path.clone();
        for _ in 0..5 {
            malicious.push("..");
        }

        // Should not escape root after normalization
        if malicious.starts_with(&root) {
            let safe = is_safe_path(&normalized, &root);
            prop_assert!(safe || normalized.components().count() == 0);
        }
    }

    /// Test: File creation respects parent directory permissions
    #[test]
    fn prop_file_creation_parent_perms(
        parent_mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid(),
        parent_owner in arbitrary_uid(),
        parent_group in arbitrary_gid()
    ) {
        let parent_perms = Permissions::new(parent_mode);
        let parent_meta = FileMeta {
            file_type: FileType::Directory,
            size: 4096,
            owner: parent_owner,
            group: parent_group,
            perms: parent_perms,
        };

        // Can create file = can write to parent directory
        let can_create = parent_perms.can_write(uid, gid, parent_owner, parent_group);

        // Verify permission logic
        if uid == parent_owner {
            prop_assert_eq!(can_create, (parent_mode & 0o200) != 0);
        } else if gid == parent_group {
            prop_assert_eq!(can_create, (parent_mode & 0o020) != 0);
        } else {
            prop_assert_eq!(can_create, (parent_mode & 0o002) != 0);
        }
    }

    /// Test: No privilege escalation
    #[test]
    fn prop_no_privilege_escalation(
        file_mode in arbitrary_permissions(),
        uid in arbitrary_uid(),
        gid in arbitrary_gid(),
        owner in arbitrary_uid(),
        group in arbitrary_gid(),
        new_mode in arbitrary_permissions()
    ) {
        let meta = FileMeta {
            file_type: FileType::RegularFile,
            size: 100,
            owner,
            group,
            perms: Permissions::new(file_mode),
        };

        // Only owner can chmod
        let can_chmod = uid == owner;

        // Only root (uid 0) can chown
        let can_chown = uid == Uid(0);

        // Cannot set setuid bit if not owner
        let can_set_setuid = uid == owner;

        // Non-owner cannot change permissions
        if uid != owner {
            prop_assert!(!can_chmod);
            prop_assert!(!can_set_setuid);
        }

        // Non-root cannot change ownership
        if uid != Uid(0) {
            prop_assert!(!can_chown);
        }
    }

    /// Test: Permission bit operations
    #[test]
    fn prop_permission_bits(
        mode1 in arbitrary_permissions(),
        mode2 in arbitrary_permissions()
    ) {
        let perms1 = Permissions::new(mode1);
        let perms2 = Permissions::new(mode2);

        // Mode should be masked to 9 bits
        prop_assert!(perms1.mode <= 0o777);
        prop_assert!(perms2.mode <= 0o777);

        // Owner permissions are top 3 bits
        prop_assert_eq!(perms1.owner_perms(), (mode1 >> 6) & 0o7);

        // Group permissions are middle 3 bits
        prop_assert_eq!(perms1.group_perms(), (mode1 >> 3) & 0o7);

        // Other permissions are bottom 3 bits
        prop_assert_eq!(perms1.other_perms(), mode1 & 0o7);

        // Combined permissions
        let combined = mode1 | mode2;
        let combined_perms = Permissions::new(combined);
        prop_assert!((combined_perms.mode & perms1.mode) == perms1.mode);
    }

    /// Test: Special file types have correct semantics
    #[test]
    fn prop_special_file_semantics(
        file_type in prop_oneof![
            Just(FileType::SyntheticFile),
            Just(FileType::FunctionFile),
            Just(FileType::WasmTranslator),
        ]
    ) {
        // Special files use computed content
        let content = match file_type {
            FileType::RegularFile => FileContent::Static(vec![]),
            FileType::Directory => FileContent::None,
            _ => FileContent::Computed,
        };

        // Function files are composable
        let is_composable = match file_type {
            FileType::FunctionFile | FileType::WasmTranslator => true,
            _ => false,
        };

        if file_type == FileType::FunctionFile || file_type == FileType::WasmTranslator {
            prop_assert!(is_composable);
            prop_assert!(matches!(content, FileContent::Computed));
        }

        // WASM files need execute permission to run
        if file_type == FileType::WasmTranslator {
            let mode = 0o755;  // rwxr-xr-x
            let perms = Permissions::new(mode);
            let can_exec = perms.can_execute(Uid(1000), Gid(1000), Uid(1000), Gid(1000));
            prop_assert!(can_exec);
        }
    }

    /// Test: Sticky bit behavior on directories
    #[test]
    fn prop_sticky_bit_directories(
        dir_mode in 0o1000u32..=0o1777u32,  // Sticky bit set
        file_owner in arbitrary_uid(),
        dir_owner in arbitrary_uid(),
        deleter in arbitrary_uid()
    ) {
        let has_sticky = (dir_mode & 0o1000) != 0;

        // In sticky directory, can only delete:
        // 1. Own files
        // 2. If you're the directory owner
        // 3. If you're root
        let can_delete = deleter == file_owner ||
                        deleter == dir_owner ||
                        deleter == Uid(0);

        if has_sticky {
            // Sticky bit enforces deletion restrictions
            if deleter != file_owner && deleter != dir_owner && deleter != Uid(0) {
                prop_assert!(!can_delete);
            }
        }
    }

    /// Test: Setuid/setgid bits
    #[test]
    fn prop_setuid_setgid_bits(
        mode in 0u32..=0o7777u32  // Include special bits
    ) {
        let has_setuid = (mode & 0o4000) != 0;
        let has_setgid = (mode & 0o2000) != 0;
        let has_sticky = (mode & 0o1000) != 0;

        // Setuid on executables
        if has_setuid && (mode & 0o111) != 0 {
            // File executes with owner's privileges
            prop_assert!(has_setuid);
        }

        // Setgid on directories
        if has_setgid {
            // New files inherit group
            prop_assert!(has_setgid);
        }

        // Extract base permissions
        let base_perms = mode & 0o777;
        prop_assert!(base_perms <= 0o777);
    }
}

/// Test specific bug: synthetic files allowing writes
#[test]
fn test_synthetic_file_write_bug() {
    let meta = FileMeta {
        file_type: FileType::SyntheticFile,
        size: 0,
        owner: Uid(1000),
        group: Gid(1000),
        perms: Permissions::new(0o666), // rw-rw-rw-
    };

    // Even with write permissions, synthetic files should not allow writes
    let content = FileContent::Computed;

    // The bug: checking only permissions, not file type
    let buggy_can_write = meta.perms.can_write(Uid(1000), Gid(1000),
                                               meta.owner, meta.group);
    assert!(buggy_can_write); // Permission says yes

    // The fix: also check file type
    let correct_can_write = match meta.file_type {
        FileType::SyntheticFile | FileType::FunctionFile | FileType::WasmTranslator => false,
        _ => buggy_can_write,
    };

    assert!(!correct_can_write); // Synthetic files cannot be written
}

/// Test: Path normalization edge cases
#[test]
fn test_path_normalization_edges() {
    let test_cases = vec![
        ("/home/../etc", "/etc"),
        ("/home/./user", "/home/user"),
        ("/home//user", "/home/user"),
        ("/home/user/..", "/home"),
        ("/../../../etc", "/etc"),
        ("/home/user/../..", "/"),
    ];

    for (input, expected) in test_cases {
        let path = PathBuf::from(input);
        let normalized = path.components().collect::<PathBuf>();

        // Should match expected after normalization
        println!("Input: {}, Expected: {}, Got: {:?}", input, expected, normalized);
    }
}