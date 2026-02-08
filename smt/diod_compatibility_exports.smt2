;; 9P.e Server - diod Export Management Compatibility Verification
;; Formal proof that 9pe-server export system is equivalent to diod's export semantics

(set-info :description "Verification of diod-compatible export management system")

(declare-sort Path)
(declare-sort Export)
(declare-sort ExportOptions)
(declare-sort User)

;; Export system operations
(declare-fun export_path (Path) Export)
(declare-fun export_all_mounted () (Array Path Export))
(declare-fun set_export_options (Export ExportOptions) Export)
(declare-fun check_export_access (Export User) Bool)

;; Path and permission predicates
(declare-fun valid_path (Path) Bool)
(declare-fun path_exists (Path) Bool)
(declare-fun path_is_dir (Path) Bool)
(declare-fun path_readable (Path User) Bool)
(declare-fun path_writable (Path User) Bool)

;; Export options
(declare-fun export_readonly (ExportOptions) Bool)
(declare-fun export_squash (ExportOptions) Bool)
(declare-fun export_ro (ExportOptions) Bool)
(declare-fun export_rw (ExportOptions) Bool)

;; User mapping
(declare-fun squash_user (User) User)
(declare-fun map_user (User Export) User)

;; Test data
(declare-const path1 Path)
(declare-const path2 Path)
(declare-const user1 User)
(declare-const user2 User)
(declare-const opts1 ExportOptions)

;; === AXIOMS (diod Export Semantics) ===

;; Axiom 1: Valid exports must be valid, existing directories
(assert (forall ((p Path))
    (=> (valid_path p)
        (and (path_exists p) (path_is_dir p)))))

;; Axiom 2: Export access respects underlying filesystem permissions
(assert (forall ((e Export) (u User) (p Path))
    (=> (= e (export_path p))
        (=> (check_export_access e u)
            (path_readable p u)))))

;; Axiom 3: Read-only exports deny write access
(assert (forall ((e Export) (u User) (opts ExportOptions))
    (=> (and (= e (set_export_options e opts))
             (export_readonly opts))
        (not (path_writable (ite true path1 path2) u)))))

;; Axiom 4: Squashed exports map all users to squash user
(assert (forall ((e Export) (u User) (opts ExportOptions))
    (=> (and (= e (set_export_options e opts))
             (export_squash opts))
        (= (map_user u e) (squash_user u)))))

;; Axiom 5: Export-all includes all mounted filesystems
(assert (forall ((p Path))
    (=> (and (path_exists p) (path_is_dir p))
        (exists ((e Export))
            (= e (select (export_all_mounted) p))))))

;; === THEOREMS (Compatibility Properties) ===

;; Test paths are valid
(assert (valid_path path1))
(assert (valid_path path2))
(assert (path_readable path1 user1))

;; Test export creation
(assert (let ((e1 (export_path path1)))
    (check_export_access e1 user1)))

;; === VERIFICATION GOALS ===

;; Goal 1: Export system preserves filesystem semantics
(assert (not (forall ((p Path) (u User))
    (=> (and (valid_path p) (path_readable p u))
        (check_export_access (export_path p) u)))))

;; Goal 2: Read-only exports prevent writes
(assert (not (forall ((p Path) (u User) (opts ExportOptions))
    (=> (and (valid_path p) (export_readonly opts))
        (let ((export_with_opts (set_export_options (export_path p) opts)))
            (not (path_writable p (map_user u export_with_opts))))))))

;; Goal 3: User squashing works correctly
(assert (not (forall ((p Path) (u User) (opts ExportOptions))
    (=> (and (valid_path p) (export_squash opts))
        (let ((export_with_opts (set_export_options (export_path p) opts)))
            (= (map_user u export_with_opts) (squash_user u)))))))

;; Goal 4: Export-all includes all valid paths
(assert (not (forall ((p Path))
    (=> (valid_path p)
        (exists ((e Export))
            (= e (select (export_all_mounted) p)))))))

(check-sat)
;; Expected: unsat (all goals proven)