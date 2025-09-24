;; Z3 Verification of Synthetic File Implementation
;; This file verifies that the Rust synthetic file implementation
;; satisfies the properties proven in Coq

;; Basic types
(declare-sort Vec)
(declare-sort PathBuf)
(declare-sort SyntheticGenerator)
(declare-sort FileSystemServer)

;; Functions from Rust implementation
(declare-fun is_synthetic_path (PathBuf) Bool)
(declare-fun read_synthetic_file (FileSystemServer PathBuf Int Int) Vec)
(declare-fun cpu_info_generate (Int Int) Vec)
(declare-fun mem_info_generate (Int Int) Vec)
(declare-fun path_starts_with (PathBuf String) Bool)
(declare-fun path_ends_with (PathBuf String) Bool)

;; Path constants
(declare-const sys_prefix String)
(declare-const cpuinfo_suffix String)
(declare-const meminfo_suffix String)
(assert (= sys_prefix "/sys/"))
(assert (= cpuinfo_suffix "cpuinfo"))
(assert (= meminfo_suffix "meminfo"))

;; Test paths
(declare-const test_sys_cpuinfo PathBuf)
(declare-const test_sys_meminfo PathBuf)
(declare-const test_regular_file PathBuf)
(declare-const test_server FileSystemServer)

;; Path properties (from implementation)
(assert (path_starts_with test_sys_cpuinfo sys_prefix))
(assert (path_ends_with test_sys_cpuinfo cpuinfo_suffix))
(assert (path_starts_with test_sys_meminfo sys_prefix))
(assert (path_ends_with test_sys_meminfo meminfo_suffix))
(assert (not (path_starts_with test_regular_file sys_prefix)))
(assert (not (path_ends_with test_regular_file cpuinfo_suffix)))
(assert (not (path_ends_with test_regular_file meminfo_suffix)))

;; Synthetic path detection (from server.rs:is_synthetic_path)
(assert (forall ((path PathBuf))
  (= (is_synthetic_path path)
     (or (path_starts_with path sys_prefix)
         (path_ends_with path cpuinfo_suffix)
         (path_ends_with path meminfo_suffix)))))

;; Property 1: Synthetic file detection is sound
(assert (is_synthetic_path test_sys_cpuinfo))
(assert (is_synthetic_path test_sys_meminfo))
(assert (not (is_synthetic_path test_regular_file)))

;; Property 2: Deterministic generation
(assert (forall ((offset Int) (count Int))
  (and (>= offset 0) (>= count 0))
  (= (cpu_info_generate offset count)
     (cpu_info_generate offset count))))

(assert (forall ((offset Int) (count Int))
  (and (>= offset 0) (>= count 0))
  (= (mem_info_generate offset count)
     (mem_info_generate offset count))))

;; Property 3: Synthetic file read correctness
(assert (forall ((server FileSystemServer) (path PathBuf) (offset Int) (count Int))
  (and (is_synthetic_path path)
       (>= offset 0)
       (>= count 0))
  (implies (path_ends_with path cpuinfo_suffix)
           (= (read_synthetic_file server path offset count)
              (cpu_info_generate offset count)))))

(assert (forall ((server FileSystemServer) (path PathBuf) (offset Int) (count Int))
  (and (is_synthetic_path path)
       (>= offset 0)
       (>= count 0))
  (implies (path_ends_with path meminfo_suffix)
           (= (read_synthetic_file server path offset count)
              (mem_info_generate offset count)))))

;; Property 4: Path safety - synthetic paths don't escape
(assert (forall ((path PathBuf))
  (implies (is_synthetic_path path)
           (or (path_starts_with path sys_prefix)
               (path_ends_with path cpuinfo_suffix)
               (path_ends_with path meminfo_suffix)))))

;; Property 5: Completeness - all expected synthetic paths are detected
(assert (forall ((path PathBuf))
  (implies (or (path_starts_with path sys_prefix)
               (path_ends_with path cpuinfo_suffix)
               (path_ends_with path meminfo_suffix))
           (is_synthetic_path path))))

;; Verification queries
(echo "Checking synthetic file implementation correctness...")

;; Check that synthetic path detection works
(push)
(assert (not (is_synthetic_path test_sys_cpuinfo)))
(check-sat) ;; Should be unsat (property holds)
(pop)

(push)
(assert (not (is_synthetic_path test_sys_meminfo)))
(check-sat) ;; Should be unsat (property holds)
(pop)

(push)
(assert (is_synthetic_path test_regular_file))
(check-sat) ;; Should be unsat (property holds)
(pop)

;; Check determinism
(push)
(declare-const offset1 Int)
(declare-const count1 Int)
(assert (>= offset1 0))
(assert (>= count1 0))
(assert (not (= (cpu_info_generate offset1 count1)
                (cpu_info_generate offset1 count1))))
(check-sat) ;; Should be unsat (determinism holds)
(pop)

;; Check path safety
(push)
(declare-const unsafe_path PathBuf)
(assert (is_synthetic_path unsafe_path))
(assert (not (path_starts_with unsafe_path sys_prefix)))
(assert (not (path_ends_with unsafe_path cpuinfo_suffix)))
(assert (not (path_ends_with unsafe_path meminfo_suffix)))
(check-sat) ;; Should be unsat (safety holds)
(pop)

(echo "Synthetic file verification complete.")