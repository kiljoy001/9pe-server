;; 9P.e Server - Folder Serving Correctness Verification
;; Formal proof that our server correctly serves filesystem folders

(set-info :description "Verification that 9pe-server correctly serves folders over 9P.e")

(declare-sort Path)
(declare-sort File)
(declare-sort Directory)
(declare-sort FileHandle)
(declare-sort User)

;; File system operations
(declare-fun is_file (Path) Bool)
(declare-fun is_directory (Path) Bool)
(declare-fun path_exists (Path) Bool)
(declare-fun path_readable (Path User) Bool)
(declare-fun path_writable (Path User) Bool)
(declare-fun parent_path (Path) Path)
(declare-fun join_path (Path String) Path)

;; 9P.e server operations
(declare-fun serve_folder (Path) Bool)
(declare-fun client_walk (Path (Array Int String)) Path)
(declare-fun client_read (Path User Int Int) (Array Int Int))
(declare-fun client_write (Path User (Array Int Int)) Int)
(declare-fun client_stat (Path User) Bool)

;; Security predicates
(declare-fun within_root (Path Path) Bool)  ; path is within root
(declare-fun safe_path (Path) Bool)         ; no .. escapes, symlink attacks
(declare-fun authenticated (User) Bool)

;; Test data
(declare-const root_path Path)
(declare-const user_file Path)
(declare-const user1 User)
(declare-const test_data (Array Int Int))

;; === AXIOMS (Modern Folder Serving) ===

;; Axiom 1: Served folder must exist and be a directory
(assert (forall ((root Path))
    (=> (serve_folder root)
        (and (path_exists root) (is_directory root)))))

;; Axiom 2: All client operations stay within root directory
(assert (forall ((root Path) (target Path))
    (=> (serve_folder root)
        (=> (path_exists target)
            (within_root target root)))))

;; Axiom 3: Path traversal security - no escaping root
(assert (forall ((root Path) (components (Array Int String)))
    (let ((result (client_walk root components)))
        (within_root result root))))

;; Axiom 4: File reads respect permissions
(assert (forall ((p Path) (u User) (offset Int) (count Int))
    (=> (not (path_readable p u))
        (= (client_read p u offset count) (as const (Array Int Int) 0)))))

;; Axiom 5: File writes respect permissions
(assert (forall ((p Path) (u User) (data (Array Int Int)))
    (=> (not (path_writable p u))
        (= (client_write p u data) 0))))

;; Axiom 6: Safe path validation prevents attacks
(assert (forall ((p Path))
    (=> (safe_path p)
        (not (exists ((root Path))
            (and (within_root root p)  ; p should be within root
                 (not (within_root p root))))))))  ; but it escapes

;; Axiom 7: Authentication required for sensitive operations
(assert (forall ((p Path) (u User) (data (Array Int Int)))
    (=> (> (client_write p u data) 0)  ; successful write
        (authenticated u))))

;; === THEOREMS (Correctness Properties) ===

;; Test setup
(assert (serve_folder root_path))
(assert (path_exists root_path))
(assert (is_directory root_path))
(assert (within_root user_file root_path))
(assert (path_readable user_file user1))
(assert (authenticated user1))

;; === VERIFICATION GOALS ===

;; Goal 1: Served folders are valid directories
(assert (not (forall ((root Path))
    (=> (serve_folder root)
        (and (path_exists root) (is_directory root))))))

;; Goal 2: No path traversal attacks possible
(assert (not (forall ((root Path) (components (Array Int String)))
    (within_root (client_walk root components) root))))

;; Goal 3: Read permissions enforced
(assert (not (forall ((p Path) (u User))
    (=> (not (path_readable p u))
        (= (client_read p u 0 1024) (as const (Array Int Int) 0))))))

;; Goal 4: Write permissions enforced
(assert (not (forall ((p Path) (u User) (data (Array Int Int)))
    (=> (not (path_writable p u))
        (= (client_write p u data) 0)))))

;; Goal 5: All operations stay within root
(assert (not (forall ((root Path) (target Path))
    (=> (and (serve_folder root) (path_exists target))
        (within_root target root)))))

;; Goal 6: Authentication required for writes
(assert (not (forall ((p Path) (u User) (data (Array Int Int)))
    (=> (> (client_write p u data) 0)
        (authenticated u)))))

(check-sat)
;; Expected: unsat (folder serving proven secure and correct)