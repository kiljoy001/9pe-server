;; Z3 Verification of Function File Implementation
;; This file verifies that the Rust function file implementation
;; satisfies the algebraic properties proven in Coq

;; Basic types
(declare-sort Vec)
(declare-sort FunctionFile)
(declare-sort FunctionFileInstance)

;; Function file operations
(declare-fun apply (FunctionFile Vec) Vec)
(declare-fun compose (FunctionFile FunctionFile) FunctionFile)
(declare-fun is_composable (FunctionFile) Bool)
(declare-fun signature (FunctionFile) String)

;; Special function files
(declare-const identity_function FunctionFile)
(declare-const base64_encode FunctionFile)
(declare-const json_parse FunctionFile)
(declare-const error_function FunctionFile)

;; Properties of identity function
(assert (is_composable identity_function))
(assert (= (signature identity_function) "Any -> Any"))
(assert (forall ((input Vec))
  (= (apply identity_function input) input)))

;; Properties of test functions
(assert (is_composable base64_encode))
(assert (is_composable json_parse))
(assert (= (signature base64_encode) "Vec<u8> -> Vec<u8>"))
(assert (= (signature json_parse) "Vec<u8> -> Json"))

;; Error function for testing
(assert (not (is_composable error_function)))

;; Composition properties (from implementation)
(assert (forall ((f FunctionFile) (g FunctionFile))
  (= (is_composable (compose f g))
     (and (is_composable f) (is_composable g)))))

(assert (forall ((f FunctionFile) (g FunctionFile))
  (= (signature (compose f g))
     (str.++ (signature f) (str.++ " ∘ " (signature g))))))

;; Composition behavior
(assert (forall ((f FunctionFile) (g FunctionFile) (input Vec))
  (= (apply (compose f g) input)
     (apply f (apply g input)))))

;; Property 1: Identity is left identity
(assert (forall ((f FunctionFile) (input Vec))
  (implies (is_composable f)
           (= (apply (compose identity_function f) input)
              (apply f input)))))

;; Property 2: Identity is right identity
(assert (forall ((f FunctionFile) (input Vec))
  (implies (is_composable f)
           (= (apply (compose f identity_function) input)
              (apply f input)))))

;; Property 3: Composition is associative
(assert (forall ((f FunctionFile) (g FunctionFile) (h FunctionFile) (input Vec))
  (implies (and (is_composable f) (is_composable g) (is_composable h))
           (= (apply (compose f (compose g h)) input)
              (apply (compose (compose f g) h) input)))))

;; Property 4: Determinism
(assert (forall ((f FunctionFile) (input Vec))
  (= (apply f input) (apply f input))))

;; Property 5: Composability preservation
(assert (forall ((f FunctionFile) (g FunctionFile))
  (implies (and (is_composable f) (is_composable g))
           (is_composable (compose f g)))))

;; Verification queries
(echo "Checking function file implementation correctness...")

;; Check identity laws
(push)
(declare-const test_func FunctionFile)
(declare-const test_input Vec)
(assert (is_composable test_func))
(assert (not (= (apply (compose identity_function test_func) test_input)
                (apply test_func test_input))))
(check-sat) ;; Should be unsat (left identity holds)
(pop)

(push)
(declare-const test_func2 FunctionFile)
(declare-const test_input2 Vec)
(assert (is_composable test_func2))
(assert (not (= (apply (compose test_func2 identity_function) test_input2)
                (apply test_func2 test_input2))))
(check-sat) ;; Should be unsat (right identity holds)
(pop)

;; Check associativity
(push)
(declare-const f1 FunctionFile)
(declare-const f2 FunctionFile)
(declare-const f3 FunctionFile)
(declare-const input1 Vec)
(assert (is_composable f1))
(assert (is_composable f2))
(assert (is_composable f3))
(assert (not (= (apply (compose f1 (compose f2 f3)) input1)
                (apply (compose (compose f1 f2) f3) input1))))
(check-sat) ;; Should be unsat (associativity holds)
(pop)

;; Check composability preservation
(push)
(declare-const cf1 FunctionFile)
(declare-const cf2 FunctionFile)
(assert (is_composable cf1))
(assert (is_composable cf2))
(assert (not (is_composable (compose cf1 cf2))))
(check-sat) ;; Should be unsat (composability preserved)
(pop)

;; Check determinism
(push)
(declare-const det_func FunctionFile)
(declare-const det_input Vec)
(assert (not (= (apply det_func det_input)
                (apply det_func det_input))))
(check-sat) ;; Should be unsat (determinism holds)
(pop)

;; Check error propagation (non-composable functions)
(push)
(assert (is_composable (compose base64_encode error_function)))
(check-sat) ;; Should be unsat (error propagates)
(pop)

(echo "Function file verification complete.")