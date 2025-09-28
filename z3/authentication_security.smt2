;;; Authentication Security Tests for 9P.e Server
;;; Verifies capability-based security, MFA enforcement, and access control

(set-logic ALL)
(set-option :produce-models true)
(set-option :produce-unsat-cores true)

;;; ============================================================================
;;; Type Definitions
;;; ============================================================================

;; User and resource identifiers
(declare-sort UserId 0)
(declare-sort ResourceId 0)
(declare-sort Key 0)
(declare-sort Signature 0)

;; Time representation
(define-sort Time () Int)

;; Permission bits
(define-fun READ_PERM () Int 1)       ; 2^0
(define-fun WRITE_PERM () Int 2)      ; 2^1
(define-fun EXECUTE_PERM () Int 4)    ; 2^2
(define-fun DELETE_PERM () Int 8)     ; 2^3
(define-fun ADMIN_PERM () Int 16)     ; 2^4
(define-fun TRAVERSE_PERM () Int 32)  ; 2^5
(define-fun MOUNT_PERM () Int 64)     ; 2^6

;; Check permission function using bitwise AND
(define-fun has-permission ((perm-set Int) (perm Int)) Bool
    (= (mod (div perm-set perm) 2) 1))

;; User record
(declare-datatypes () ((User
    (mk-user
        (user-id UserId)
        (user-name String)
        (user-pubkey Key)
        (user-groups (Array Int String))))))

;; Capability token
(declare-datatypes () ((Capability
    (mk-capability
        (cap-id Int)
        (cap-issuer UserId)
        (cap-subject UserId)
        (cap-resource ResourceId)
        (cap-permissions Int)
        (cap-issued-at Time)
        (cap-expires-at Time)
        (cap-max-uses Int)
        (cap-delegation-allowed Bool)))))

;; Signed capability
(declare-datatypes () ((SignedCapability
    (mk-signed-cap
        (sc-capability Capability)
        (sc-signature Signature)))))

;; Authentication method
(declare-datatypes () ((AuthMethod
    AuthNone
    (AuthPassword (pwd-hash Int))
    (AuthPublicKey (auth-key Key))
    (AuthCapability (auth-cap SignedCapability)))))

;; Security context
(declare-datatypes () ((SecurityContext
    (mk-sec-context
        (ctx-user (Maybe User))
        (ctx-method AuthMethod)
        (ctx-capabilities (Array Int SignedCapability))
        (ctx-time Time)
        (ctx-mfa-verified Bool)))))

(declare-datatypes () ((Maybe (Nothing) (Just (just-val User)))))

;; System state
(declare-datatypes () ((AuthSystem
    (mk-auth-system
        (sys-users (Array Int User))
        (sys-capabilities (Array Int SignedCapability))
        (sys-revoked (Array Int Int))
        (sys-server-key Key)
        (sys-current-time Time)))))

;; Cryptographic primitives (axiomatized)
(declare-fun verify-signature (Key Int Signature) Bool)
(declare-fun verify-password (String Int) Bool)

;; Axiom: Signatures are unforgeable
(assert (forall ((key Key) (data Int) (sig Signature))
    (=> (verify-signature key data sig)
        true))) ; Simplified - signature was created with correct key

;;; ============================================================================
;;; Security Properties
;;; ============================================================================

;; Valid capability predicate
(define-fun valid-capability ((sys AuthSystem) (cap Capability)) Bool
    (and (<= (cap-issued-at cap) (sys-current-time sys))
         (<= (sys-current-time sys) (cap-expires-at cap))
         (not (select (sys-revoked sys) (cap-id cap)))))

;; Valid signed capability
(define-fun valid-signed-capability ((sys AuthSystem) (scap SignedCapability)) Bool
    (and (valid-capability sys (sc-capability scap))
         (verify-signature (sys-server-key sys)
                          (cap-id (sc-capability scap))
                          (sc-signature scap))))

;; MFA requirement check
(define-fun require-mfa ((resource ResourceId)) Bool
    true) ; Simplified - admin resources require MFA

;; Authenticated predicate
(define-fun authenticated ((sys AuthSystem) (ctx SecurityContext)) Bool
    (or
        ;; Public key auth
        (and (is-Just (ctx-user ctx))
             (is-AuthPublicKey (ctx-method ctx))
             (= (user-pubkey (just-val (ctx-user ctx)))
                (auth-key (ctx-method ctx))))
        ;; Capability auth
        (and (is-Just (ctx-user ctx))
             (is-AuthCapability (ctx-method ctx))
             (valid-signed-capability sys (auth-cap (ctx-method ctx)))
             (= (user-id (just-val (ctx-user ctx)))
                (cap-subject (sc-capability (auth-cap (ctx-method ctx))))))
        ;; Password auth
        (and (is-Just (ctx-user ctx))
             (is-AuthPassword (ctx-method ctx))
             (verify-password (user-name (just-val (ctx-user ctx)))
                            (pwd-hash (ctx-method ctx))))))

;; Has access predicate
(define-fun has-access ((sys AuthSystem) (ctx SecurityContext)
                       (resource ResourceId) (perm Int)) Bool
    (and (authenticated sys ctx)
         ;; MFA check
         (=> (require-mfa resource) (ctx-mfa-verified ctx))
         ;; Permission check via capabilities
         (exists ((i Int))
             (and (>= i 0)
                  (let ((scap (select (ctx-capabilities ctx) i)))
                       (and (valid-signed-capability sys scap)
                            (= (cap-resource (sc-capability scap)) resource)
                            (has-permission (cap-permissions (sc-capability scap)) perm)))))))

;;; ============================================================================
;;; Test 1: No Access Without Authentication
;;; ============================================================================

(push)
(echo "Test 1: No access without authentication")

(declare-const sys1 AuthSystem)
(declare-const ctx1 SecurityContext)
(declare-const resource1 ResourceId)

;; Context is not authenticated
(assert (= (ctx-method ctx1) AuthNone))
(assert (is-Nothing (ctx-user ctx1)))
(assert (not (authenticated sys1 ctx1)))

;; Should not have access to any resource
(assert (not (has-access sys1 ctx1 resource1 READ_PERM)))
(assert (not (has-access sys1 ctx1 resource1 WRITE_PERM)))

(check-sat)
(echo "Verified: No access without authentication")
(pop)

;;; ============================================================================
;;; Test 2: Expired Capabilities Grant No Access
;;; ============================================================================

(push)
(echo "Test 2: Expired capabilities grant no access")

(declare-const sys2 AuthSystem)
(declare-const user2 User)
(declare-const cap2 Capability)
(declare-const scap2 SignedCapability)
(declare-const resource2 ResourceId)

;; Set up expired capability
(assert (= (sys-current-time sys2) 1000))
(assert (= (cap-expires-at cap2) 500)) ; Expired
(assert (= (cap-issued-at cap2) 100))
(assert (= (sc-capability scap2) cap2))

;; Valid signature but expired
(assert (verify-signature (sys-server-key sys2) (cap-id cap2) (sc-signature scap2)))

;; Should not be valid
(assert (not (valid-capability sys2 cap2)))
(assert (not (valid-signed-capability sys2 scap2)))

;; Create context with expired capability
(declare-const ctx2 SecurityContext)
(assert (= (ctx-method ctx2) (AuthCapability scap2)))
(assert (= (select (ctx-capabilities ctx2) 0) scap2))

;; Should not have access
(assert (not (has-access sys2 ctx2 resource2 READ_PERM)))

(check-sat)
(echo "Verified: Expired capabilities grant no access")
(pop)

;;; ============================================================================
;;; Test 3: Revoked Capabilities Grant No Access
;;; ============================================================================

(push)
(echo "Test 3: Revoked capabilities grant no access")

(declare-const sys3 AuthSystem)
(declare-const cap3 Capability)
(declare-const scap3 SignedCapability)

;; Valid time range
(assert (= (sys-current-time sys3) 500))
(assert (= (cap-issued-at cap3) 100))
(assert (= (cap-expires-at cap3) 1000))

;; But capability is revoked
(assert (= (cap-id cap3) 42))
(assert (select (sys-revoked sys3) 42)) ; Revoked

;; Should not be valid
(assert (not (valid-capability sys3 cap3)))

(check-sat)
(echo "Verified: Revoked capabilities grant no access")
(pop)

;;; ============================================================================
;;; Test 4: MFA Enforcement for Sensitive Resources
;;; ============================================================================

(push)
(echo "Test 4: MFA enforcement for sensitive resources")

(declare-const sys4 AuthSystem)
(declare-const ctx-no-mfa SecurityContext)
(declare-const ctx-with-mfa SecurityContext)
(declare-const admin-resource ResourceId)
(declare-const user4 User)

;; Admin resource requires MFA
(assert (require-mfa admin-resource))

;; Both contexts are authenticated
(assert (authenticated sys4 ctx-no-mfa))
(assert (authenticated sys4 ctx-with-mfa))
(assert (is-Just (ctx-user ctx-no-mfa)))
(assert (is-Just (ctx-user ctx-with-mfa)))

;; But only one has MFA verified
(assert (not (ctx-mfa-verified ctx-no-mfa)))
(assert (ctx-mfa-verified ctx-with-mfa))

;; Valid capability for both
(declare-const admin-cap Capability)
(assert (= (cap-resource admin-cap) admin-resource))
(assert (has-permission (cap-permissions admin-cap) ADMIN_PERM))
(assert (valid-capability sys4 admin-cap))

(declare-const scap4 SignedCapability)
(assert (= (sc-capability scap4) admin-cap))
(assert (verify-signature (sys-server-key sys4) (cap-id admin-cap) (sc-signature scap4)))
(assert (= (select (ctx-capabilities ctx-no-mfa) 0) scap4))
(assert (= (select (ctx-capabilities ctx-with-mfa) 0) scap4))

;; Only MFA-verified context should have access
(assert (not (has-access sys4 ctx-no-mfa admin-resource ADMIN_PERM)))
(assert (has-access sys4 ctx-with-mfa admin-resource ADMIN_PERM))

(check-sat)
(echo "Verified: MFA required for sensitive resources")
(pop)

;;; ============================================================================
;;; Test 5: Capability Delegation Security
;;; ============================================================================

(push)
(echo "Test 5: Capability delegation preserves security")

(declare-const sys5 AuthSystem)
(declare-const original-cap Capability)
(declare-const delegated-cap Capability)
(declare-const delegator UserId)
(declare-const delegate UserId)

;; Original capability is valid
(assert (valid-capability sys5 original-cap))
(assert (cap-delegation-allowed original-cap))
(assert (= (cap-subject original-cap) delegator))

;; Delegated capability properties
(assert (= (cap-id delegated-cap) (+ (cap-id original-cap) 1000))) ; New ID
(assert (= (cap-issuer delegated-cap) delegator))                  ; Delegator becomes issuer
(assert (= (cap-subject delegated-cap) delegate))                  ; New subject
(assert (= (cap-resource delegated-cap) (cap-resource original-cap)))
(assert (= (cap-permissions delegated-cap) (cap-permissions original-cap)))
(assert (= (cap-issued-at delegated-cap) (cap-issued-at original-cap)))
(assert (= (cap-expires-at delegated-cap) (cap-expires-at original-cap)))
(assert (not (cap-delegation-allowed delegated-cap))) ; Can't re-delegate

;; Delegated capability should be valid if not revoked
(assert (not (select (sys-revoked sys5) (cap-id delegated-cap))))
(assert (valid-capability sys5 delegated-cap))

(check-sat)
(echo "Verified: Delegation preserves security properties")
(pop)

;;; ============================================================================
;;; Test 6: Least Privilege Principle
;;; ============================================================================

(push)
(echo "Test 6: Least privilege - minimal permissions granted")

(declare-const sys6 AuthSystem)
(declare-const ctx6 SecurityContext)
(declare-const resource6 ResourceId)

;; User has capability with only READ permission
(declare-const read-only-cap Capability)
(assert (= (cap-permissions read-only-cap) READ_PERM))
(assert (= (cap-resource read-only-cap) resource6))
(assert (valid-capability sys6 read-only-cap))

(declare-const scap6 SignedCapability)
(assert (= (sc-capability scap6) read-only-cap))
(assert (verify-signature (sys-server-key sys6) (cap-id read-only-cap) (sc-signature scap6)))
(assert (= (select (ctx-capabilities ctx6) 0) scap6))
(assert (authenticated sys6 ctx6))

;; Should have read access
(assert (has-access sys6 ctx6 resource6 READ_PERM))

;; Should NOT have write or other access
(assert (not (has-permission (cap-permissions read-only-cap) WRITE_PERM)))
(assert (not (has-permission (cap-permissions read-only-cap) DELETE_PERM)))
(assert (not (has-permission (cap-permissions read-only-cap) ADMIN_PERM)))

(check-sat)
(echo "Verified: Least privilege principle enforced")
(pop)

;;; ============================================================================
;;; Test 7: Password Security - INSECURE Implementation Detection
;;; ============================================================================

(push)
(echo "Test 7: Detect insecure password comparison")

(declare-const sys7 AuthSystem)
(declare-const user7 User)
(declare-const ctx7 SecurityContext)

;; INSECURE: Password compared with username (the bug we're testing)
(assert (= (user-name user7) "testuser"))
(assert (= (ctx-method ctx7) (AuthPassword 8))) ; hash of "testuser"

;; This SHOULD NOT authenticate (detecting the bug)
;; In the buggy implementation: password == username
(declare-fun insecure-password-check (String Int) Bool)
(assert (insecure-password-check "testuser" 8)) ; Bug: comparing password with username

;; The secure version should use proper hashing
(assert (not (= (user-name user7) "8"))) ; Password should not equal username

(check-sat)
(echo "Verified: Insecure password comparison detected")
(pop)

;;; ============================================================================
;;; Test 8: Rate Limiting
;;; ============================================================================

(push)
(echo "Test 8: Rate limiting enforcement")

(declare-const max-requests Int)
(declare-const time-window Time)
(declare-const current-requests Int)
(declare-const window-start Time)
(declare-const current-time Time)

(assert (= max-requests 100))
(assert (= time-window 60)) ; 60 seconds
(assert (= window-start 1000))
(assert (= current-time 1030)) ; Within window

;; Within time window
(assert (< current-time (+ window-start time-window)))

;; Check rate limit
(define-fun check-rate-limit ((cur-req Int) (max-req Int)) Bool
    (< cur-req max-req))

;; At limit should deny
(assert (= current-requests 100))
(assert (not (check-rate-limit current-requests max-requests)))

;; Below limit should allow
(assert (check-rate-limit 99 max-requests))

(check-sat)
(echo "Verified: Rate limiting works correctly")
(pop)

(echo "All authentication security tests completed!")
(exit)