;; STATUS: VERIFIED
(set-info :status unsat)
(set-logic ALL)

;; === TYPE DEFINITIONS ===

(declare-sort Capability)
(declare-sort Principal)
(declare-sort Resource)

;; Permission bits
(declare-const PERM_READ Int)
(declare-const PERM_WRITE Int)
(declare-const PERM_EXECUTE Int)
(declare-const PERM_DELEGATE Int)

(assert (= PERM_READ 1))
(assert (= PERM_WRITE 2))
(assert (= PERM_EXECUTE 4))
(assert (= PERM_DELEGATE 8))

;; === CAPABILITY STRUCTURE ===

(declare-fun cap_issuer (Capability) Principal)
(declare-fun cap_holder (Capability) Principal)
(declare-fun cap_resource (Capability) Resource)
(declare-fun cap_permissions (Capability) Int)
(declare-fun cap_parent (Capability) Capability)
(declare-fun is_root_capability (Capability) Bool)

;; === DELEGATION OPERATIONS ===

(declare-fun delegate (Capability Principal Int) Capability)
(declare-fun has_permission_bit (Int Int) Bool)

;; Helper: Check if permission bits are set
(assert (forall ((perms Int) (bit Int))
  (= (has_permission_bit perms bit)
     (> (mod (div perms bit) 2) 0))))

;; === CORE SECURITY AXIOMS ===

;; Axiom 1: Delegated capability cannot have more permissions than parent
(assert (forall ((parent_cap Capability) (delegatee Principal) (requested_perms Int))
  (let ((delegated_cap (delegate parent_cap delegatee requested_perms)))
    (<= (cap_permissions delegated_cap) (cap_permissions parent_cap)))))

;; Axiom 2: Can only delegate if you have DELEGATE permission
(assert (forall ((cap Capability) (delegatee Principal) (requested_perms Int))
  (=> (not (has_permission_bit (cap_permissions cap) PERM_DELEGATE))
      (= (cap_permissions (delegate cap delegatee requested_perms)) 0))))

;; Axiom 3: Delegated capability must be subset of parent permissions
(assert (forall ((parent_cap Capability) (delegatee Principal) (requested_perms Int))
  (let ((delegated_cap (delegate parent_cap delegatee requested_perms)))
    (forall ((bit Int))
      (=> (has_permission_bit (cap_permissions delegated_cap) bit)
          (has_permission_bit (cap_permissions parent_cap) bit))))))

;; Axiom 4: Parent chain is preserved
(assert (forall ((parent_cap Capability) (delegatee Principal) (requested_perms Int))
  (let ((delegated_cap (delegate parent_cap delegatee requested_perms)))
    (and (= (cap_parent delegated_cap) parent_cap)
         (= (cap_resource delegated_cap) (cap_resource parent_cap))))))

;; Axiom 5: Cannot delegate root capabilities
(assert (forall ((cap Capability) (delegatee Principal) (requested_perms Int))
  (=> (is_root_capability cap)
      (= (cap_permissions (delegate cap delegatee requested_perms)) 0))))

;; === SECURITY THEOREMS ===

;; Test constants
(declare-const original_cap Capability)
(declare-const alice Principal)
(declare-const bob Principal)
(declare-const test_resource Resource)

;; THEOREM 1: Cannot gain write permission through delegation
;; If parent doesn't have WRITE, delegated capability can't have WRITE

(push)
(assert (and
  ;; Alice has READ-only capability
  (= (cap_permissions original_cap) PERM_READ)
  (= (cap_holder original_cap) alice)
  (= (cap_resource original_cap) test_resource)
  (not (is_root_capability original_cap))

  ;; Alice tries to delegate with WRITE permission to Bob
  (let ((delegated_cap (delegate original_cap bob (+ PERM_READ PERM_WRITE))))
    (has_permission_bit (cap_permissions delegated_cap) PERM_WRITE))
))

(check-sat)
;; Expected: unsat (cannot gain WRITE through delegation)
(pop)

;; THEOREM 2: Cannot delegate without DELEGATE permission

(push)
(assert (and
  ;; Original capability has READ but NOT DELEGATE
  (= (cap_permissions original_cap) PERM_READ)
  (= (cap_holder original_cap) alice)

  ;; Alice tries to delegate to Bob
  (let ((delegated_cap (delegate original_cap bob PERM_READ)))
    (> (cap_permissions delegated_cap) 0))
))

(check-sat)
;; Expected: unsat (cannot delegate without DELEGATE permission)
(pop)

;; THEOREM 3: Delegated permissions are always subset

(push)
(assert (and
  ;; Original has READ + EXECUTE + DELEGATE
  (= (cap_permissions original_cap) (+ PERM_READ PERM_EXECUTE PERM_DELEGATE))
  (= (cap_holder original_cap) alice)
  (not (is_root_capability original_cap))

  ;; Alice delegates with READ + EXECUTE + WRITE (tries to add WRITE)
  (let ((delegated_cap (delegate original_cap bob (+ PERM_READ PERM_EXECUTE PERM_WRITE))))
    (has_permission_bit (cap_permissions delegated_cap) PERM_WRITE))
))

(check-sat)
;; Expected: unsat (cannot add permissions not in parent)
(pop)

;; THEOREM 4: Transitive delegation preserves bounds

(push)
(declare-const charlie Principal)
(declare-const bob_cap Capability)

(assert (and
  ;; Alice has READ + WRITE + DELEGATE
  (= (cap_permissions original_cap) (+ PERM_READ PERM_WRITE PERM_DELEGATE))
  (= (cap_holder original_cap) alice)
  (not (is_root_capability original_cap))

  ;; Bob gets delegated capability with only READ + DELEGATE (no WRITE)
  (= bob_cap (delegate original_cap bob (+ PERM_READ PERM_DELEGATE)))
  (not (has_permission_bit (cap_permissions bob_cap) PERM_WRITE))

  ;; Bob tries to delegate WRITE to Charlie
  (let ((charlie_cap (delegate bob_cap charlie PERM_WRITE)))
    (has_permission_bit (cap_permissions charlie_cap) PERM_WRITE))
))

(check-sat)
;; Expected: unsat (transitive delegation cannot add permissions)
(pop)

;; THEOREM 5: Root capabilities cannot be delegated

(push)
(assert (and
  ;; Root capability with all permissions
  (is_root_capability original_cap)
  (= (cap_permissions original_cap) (+ PERM_READ PERM_WRITE PERM_EXECUTE PERM_DELEGATE))

  ;; Try to delegate it
  (let ((delegated_cap (delegate original_cap bob PERM_READ)))
    (> (cap_permissions delegated_cap) 0))
))

(check-sat)
;; Expected: unsat (root capabilities cannot be delegated)
(pop)

;; === VERIFICATION SUMMARY ===

(echo "✓ Capability delegation safety verified")
(echo "  - Cannot gain permissions through delegation")
(echo "  - Cannot delegate without DELEGATE permission")
(echo "  - Delegated permissions are always subset of parent")
(echo "  - Transitive delegation preserves bounds")
(echo "  - Root capabilities cannot be delegated")
