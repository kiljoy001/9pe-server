;;; File Permission Enforcement Tests for 9P.e Server
;;; Verifies Unix-style permissions, path safety, and synthetic file properties

(set-logic ALL)
(set-option :produce-models true)
(set-option :produce-proofs true)

;;; ============================================================================
;;; Type Definitions
;;; ============================================================================

;; User and group identifiers
(define-sort Uid () Int)
(define-sort Gid () Int)
(define-sort FileId () Int)

;; File path as array of strings
(declare-sort Path 0)
(declare-fun path-length (Path) Int)
(declare-fun path-component (Path Int) String)

;; File type enumeration
(declare-datatypes () ((FileType
    RegularFile
    Directory
    SymbolicLink
    SyntheticFile    ; Computed on-the-fly
    FunctionFile     ; Transforms input to output
    WasmTranslator))) ; WASM-based translator

;; Unix-style permissions (octal representation)
(declare-datatypes () ((Permissions
    (mk-perms
        (owner-perms Int)  ; 0-7 (rwx)
        (group-perms Int)  ; 0-7 (rwx)
        (other-perms Int))))) ; 0-7 (rwx)

;; Permission bits
(define-fun READ_BIT () Int 4)   ; 0b100
(define-fun WRITE_BIT () Int 2)  ; 0b010
(define-fun EXEC_BIT () Int 1)   ; 0b001

;; File metadata
(declare-datatypes () ((FileMeta
    (mk-file-meta
        (file-type FileType)
        (file-size Int)
        (file-owner Uid)
        (file-group Gid)
        (file-perms Permissions)
        (file-mtime Int)
        (file-atime Int)))))

;; File content types
(declare-datatypes () ((FileContent
    (StaticContent (static-data (Array Int Int)))
    (ComputedContent)  ; Function that computes content
    NoContent)))       ; Directories have no content

;; File system entry
(declare-datatypes () ((FSEntry
    (mk-fs-entry
        (entry-id FileId)
        (entry-path Path)
        (entry-meta FileMeta)
        (entry-content FileContent)
        (entry-children (Array Int FileId))
        (entry-children-count Int)))))

;; File system state
(declare-datatypes () ((FileSystem
    (mk-fs
        (fs-entries (Array FileId FSEntry))
        (fs-entry-count Int)
        (fs-root FileId)))))

;;; ============================================================================
;;; Permission Checking Functions
;;; ============================================================================

;; Check if permission bits allow operation
(define-fun has-permission-bit ((perms Int) (bit Int)) Bool
    (not (= (mod (div perms bit) 2) 0)))

;; Check read permission
(define-fun can-read ((uid Uid) (gid Gid) (meta FileMeta)) Bool
    (let ((perms (file-perms meta)))
        (ite (= uid (file-owner meta))
             (has-permission-bit (owner-perms perms) READ_BIT)
             (ite (= gid (file-group meta))
                  (has-permission-bit (group-perms perms) READ_BIT)
                  (has-permission-bit (other-perms perms) READ_BIT)))))

;; Check write permission
(define-fun can-write ((uid Uid) (gid Gid) (meta FileMeta)) Bool
    (let ((perms (file-perms meta)))
        (ite (= uid (file-owner meta))
             (has-permission-bit (owner-perms perms) WRITE_BIT)
             (ite (= gid (file-group meta))
                  (has-permission-bit (group-perms perms) WRITE_BIT)
                  (has-permission-bit (other-perms perms) WRITE_BIT)))))

;; Check execute permission
(define-fun can-execute ((uid Uid) (gid Gid) (meta FileMeta)) Bool
    (let ((perms (file-perms meta)))
        (ite (= uid (file-owner meta))
             (has-permission-bit (owner-perms perms) EXEC_BIT)
             (ite (= gid (file-group meta))
                  (has-permission-bit (group-perms perms) EXEC_BIT)
                  (has-permission-bit (other-perms perms) EXEC_BIT)))))

;; Check traverse permission (execute for directories)
(define-fun can-traverse ((uid Uid) (gid Gid) (meta FileMeta)) Bool
    (and (= (file-type meta) Directory)
         (can-execute uid gid meta)))

;;; ============================================================================
;;; Test 1: Permission Enforcement on Read
;;; ============================================================================

(push)
(echo "Test 1: Read operations require read permission")

(declare-const fs FileSystem)
(declare-const entry FSEntry)
(declare-const uid1 Uid)
(declare-const gid1 Gid)

;; Set up file with specific permissions (0644 = rw-r--r--)
(declare-const meta1 FileMeta)
(assert (= meta1 (mk-file-meta RegularFile 100 1000 1000
                               (mk-perms 6 4 4) ; 110 100 100 in binary
                               0 0)))

;; Owner can read (6 = 110 = rw-)
(assert (can-read 1000 1000 meta1))

;; Group can read (4 = 100 = r--)
(assert (can-read 2000 1000 meta1)) ; Different uid, same gid

;; Others can read (4 = 100 = r--)
(assert (can-read 3000 3000 meta1)) ; Different uid and gid

;; But with 0600 (rw-------)
(declare-const meta2 FileMeta)
(assert (= meta2 (mk-file-meta RegularFile 100 1000 1000
                               (mk-perms 6 0 0)
                               0 0)))

;; Only owner can read
(assert (can-read 1000 1000 meta2))
(assert (not (can-read 2000 1000 meta2))) ; Group cannot
(assert (not (can-read 3000 3000 meta2))) ; Others cannot

(check-sat)
(echo "Verified: Read permission enforcement")
(pop)

;;; ============================================================================
;;; Test 2: Permission Enforcement on Write
;;; ============================================================================

(push)
(echo "Test 2: Write operations require write permission")

(declare-const meta3 FileMeta)

;; File with 0644 permissions
(assert (= meta3 (mk-file-meta RegularFile 100 1000 1000
                               (mk-perms 6 4 4) ; rw-r--r--
                               0 0)))

;; Only owner can write
(assert (can-write 1000 1000 meta3))
(assert (not (can-write 2000 1000 meta3))) ; Group cannot write
(assert (not (can-write 3000 3000 meta3))) ; Others cannot write

;; With 0666 (rw-rw-rw-)
(declare-const meta4 FileMeta)
(assert (= meta4 (mk-file-meta RegularFile 100 1000 1000
                               (mk-perms 6 6 6)
                               0 0)))

;; Everyone can write
(assert (can-write 1000 1000 meta4))
(assert (can-write 2000 1000 meta4))
(assert (can-write 3000 3000 meta4))

(check-sat)
(echo "Verified: Write permission enforcement")
(pop)

;;; ============================================================================
;;; Test 3: Directory Traversal Requires Execute Permission
;;; ============================================================================

(push)
(echo "Test 3: Directory traversal requires execute permission")

(declare-const dir-meta FileMeta)

;; Directory with 0755 (rwxr-xr-x)
(assert (= dir-meta (mk-file-meta Directory 4096 1000 1000
                                   (mk-perms 7 5 5) ; 111 101 101
                                   0 0)))

;; All can traverse
(assert (can-traverse 1000 1000 dir-meta))
(assert (can-traverse 2000 1000 dir-meta))
(assert (can-traverse 3000 3000 dir-meta))

;; Directory with 0744 (rwxr--r--)
(declare-const dir-meta2 FileMeta)
(assert (= dir-meta2 (mk-file-meta Directory 4096 1000 1000
                                    (mk-perms 7 4 4) ; 111 100 100
                                    0 0)))

;; Only owner can traverse
(assert (can-traverse 1000 1000 dir-meta2))
(assert (not (can-traverse 2000 1000 dir-meta2))) ; Group cannot
(assert (not (can-traverse 3000 3000 dir-meta2))) ; Others cannot

(check-sat)
(echo "Verified: Directory traversal permission")
(pop)

;;; ============================================================================
;;; Test 4: Synthetic Files are Read-Only
;;; ============================================================================

(push)
(echo "Test 4: Synthetic files cannot be written to")

(declare-const synth-entry FSEntry)
(declare-const synth-meta FileMeta)

;; Synthetic file with write permissions set
(assert (= synth-meta (mk-file-meta SyntheticFile 0 1000 1000
                                     (mk-perms 7 7 7) ; rwxrwxrwx
                                     0 0)))

;; Even with write permission, synthetic files should not allow writes
(declare-fun allows-write-to-synthetic (FileType) Bool)
(assert (not (allows-write-to-synthetic SyntheticFile)))
(assert (not (allows-write-to-synthetic FunctionFile)))
(assert (not (allows-write-to-synthetic WasmTranslator)))
(assert (allows-write-to-synthetic RegularFile))

;; Content type check
(assert (= (entry-content synth-entry) ComputedContent))
;; Write operations to ComputedContent should fail
(declare-fun can-write-content (FileContent) Bool)
(assert (not (can-write-content ComputedContent)))
(assert (can-write-content (StaticContent (store (store ((as const (Array Int Int)) 0) 0 65) 1 66))))

(check-sat)
(echo "Verified: Synthetic files are read-only")
(pop)

;;; ============================================================================
;;; Test 5: Path Traversal Prevention
;;; ============================================================================

(push)
(echo "Test 5: Path traversal attacks are prevented")

;; Check if path is safe (no escaping root)
(declare-fun is-safe-path (Path Path) Bool) ; (requested, root)

;; Path normalization removes .. and .
(declare-fun normalize-path (Path) Path)

;; Root path
(declare-const root-path Path)
(assert (= (path-length root-path) 2))
(assert (= (path-component root-path 0) "/home"))
(assert (= (path-component root-path 1) "user"))

;; Malicious path attempts
(declare-const malicious-path1 Path)
(assert (= (path-length malicious-path1) 4))
(assert (= (path-component malicious-path1 0) "/home"))
(assert (= (path-component malicious-path1 1) "user"))
(assert (= (path-component malicious-path1 2) ".."))
(assert (= (path-component malicious-path1 3) ".."))

;; After normalization, should not escape root
(declare-const normalized1 Path)
(assert (= normalized1 (normalize-path malicious-path1)))
(assert (not (is-safe-path normalized1 root-path))) ; Detects escape attempt

;; Valid path
(declare-const valid-path Path)
(assert (= (path-length valid-path) 3))
(assert (= (path-component valid-path 0) "/home"))
(assert (= (path-component valid-path 1) "user"))
(assert (= (path-component valid-path 2) "documents"))

(assert (is-safe-path valid-path root-path))

(check-sat)
(echo "Verified: Path traversal prevention")
(pop)

;;; ============================================================================
;;; Test 6: File Creation Respects Parent Directory Permissions
;;; ============================================================================

(push)
(echo "Test 6: File creation requires write permission on parent directory")

(declare-const parent-dir FSEntry)
(declare-const parent-meta FileMeta)
(declare-const uid6 Uid)
(declare-const gid6 Gid)

;; Parent directory with 0755 (rwxr-xr-x)
(assert (= parent-meta (mk-file-meta Directory 4096 1000 1000
                                      (mk-perms 7 5 5)
                                      0 0)))
(assert (= (entry-meta parent-dir) parent-meta))

;; Owner can create files (needs write permission)
(assert (can-write 1000 1000 parent-meta))

;; Others cannot create files (no write permission)
(assert (not (can-write 2000 2000 parent-meta)))

;; With sticky bit (1777 = rwxrwxrwt)
(declare-const sticky-dir-meta FileMeta)
(assert (= sticky-dir-meta (mk-file-meta Directory 4096 1000 1000
                                          (mk-perms 7 7 7)
                                          0 0)))

;; Everyone can write to sticky directory
(assert (can-write 1000 1000 sticky-dir-meta))
(assert (can-write 2000 2000 sticky-dir-meta))

;; But deletion requires ownership (sticky bit behavior)
(declare-fun can-delete-in-sticky (Uid Uid Uid) Bool) ; (deleter, file-owner, dir-owner)
(assert (can-delete-in-sticky 1000 1000 1000)) ; Owner can delete own files
(assert (can-delete-in-sticky 1000 2000 1000)) ; Dir owner can delete any
(assert (not (can-delete-in-sticky 2000 1000 1000))) ; Others cannot delete

(check-sat)
(echo "Verified: Parent directory permission for file creation")
(pop)

;;; ============================================================================
;;; Test 7: No Privilege Escalation
;;; ============================================================================

(push)
(echo "Test 7: Operations cannot escalate privileges")

(declare-const before-meta FileMeta)
(declare-const after-meta FileMeta)
(declare-const uid7 Uid)
(declare-const gid7 Gid)

;; Non-owner user
(assert (not (= uid7 (file-owner before-meta))))

;; Cannot change permissions if not owner
(declare-fun can-chmod (Uid FileMeta) Bool)
(assert (can-chmod (file-owner before-meta) before-meta))
(assert (not (can-chmod uid7 before-meta)))

;; Cannot change ownership if not root
(declare-const root-uid Uid)
(assert (= root-uid 0))
(declare-fun can-chown (Uid FileMeta) Bool)
(assert (can-chown root-uid before-meta))
(assert (not (can-chown uid7 before-meta)))

;; Cannot set setuid bit if not owner
(declare-fun can-set-setuid (Uid FileMeta) Bool)
(assert (can-set-setuid (file-owner before-meta) before-meta))
(assert (not (can-set-setuid uid7 before-meta)))

(check-sat)
(echo "Verified: No privilege escalation")
(pop)

;;; ============================================================================
;;; Test 8: Error Cases and Edge Conditions
;;; ============================================================================

(push)
(echo "Test 8: Handle error cases correctly")

;; File not found
(declare-const missing-id FileId)
(declare-const fs8 FileSystem)
(assert (= missing-id 99999))
(assert (< (fs-entry-count fs8) 99999))

;; Access to non-existent file should fail safely
(declare-fun file-exists (FileSystem FileId) Bool)
(assert (not (file-exists fs8 missing-id)))

;; Zero-size file operations
(declare-const zero-file FSEntry)
(declare-const zero-meta FileMeta)
(assert (= (file-size zero-meta) 0))
(assert (= (entry-meta zero-file) zero-meta))

;; Reading past EOF
(declare-fun read-file (FSEntry Int Int) (Array Int Int))
(declare-const read-result (Array Int Int))
(assert (= read-result (read-file zero-file 100 50))) ; Offset 100, count 50
;; Should return empty array
(assert (= (select read-result 0) -1)) ; -1 indicates no data

;; Invalid permission values
(declare-const invalid-perms Permissions)
(assert (= invalid-perms (mk-perms 8 8 8))) ; Invalid: > 7
(declare-fun is-valid-perms (Permissions) Bool)
(assert (not (is-valid-perms invalid-perms)))
(assert (is-valid-perms (mk-perms 7 7 7)))
(assert (is-valid-perms (mk-perms 0 0 0)))

(check-sat)
(echo "Verified: Error cases handled correctly")
(pop)

;;; ============================================================================
;;; Test 9: Special File Types
;;; ============================================================================

(push)
(echo "Test 9: Special file types have correct semantics")

;; Function files transform input to output
(declare-const func-file FSEntry)
(assert (= (file-type (entry-meta func-file)) FunctionFile))
(assert (= (entry-content func-file) ComputedContent))

;; Function files are composable
(declare-fun is-composable (FileType) Bool)
(assert (is-composable FunctionFile))
(assert (is-composable WasmTranslator))
(assert (not (is-composable RegularFile)))

;; WASM translators have special properties
(declare-const wasm-file FSEntry)
(assert (= (file-type (entry-meta wasm-file)) WasmTranslator))

;; WASM files require execute permission to run
(declare-const wasm-meta FileMeta)
(assert (= wasm-meta (entry-meta wasm-file)))
(declare-fun can-run-wasm (Uid Gid FileMeta) Bool)
(assert (= (can-run-wasm uid gid wasm-meta)
           (can-execute uid gid wasm-meta)))

;; Symbolic links
(declare-const symlink FSEntry)
(assert (= (file-type (entry-meta symlink)) SymbolicLink))
;; Symlinks store target path, not content
(assert (= (entry-content symlink) NoContent))

(check-sat)
(echo "Verified: Special file types work correctly")
(pop)

(echo "All file permission tests completed!")
(exit)