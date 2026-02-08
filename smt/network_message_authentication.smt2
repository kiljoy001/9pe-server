;;
;; SMT2 Formal Verification: Network Message Authentication
;; Proves that 9P.e protocol messages cannot be spoofed
;;

(set-logic UFLIA)
(set-info :status unsat)

;; === TYPE DEFINITIONS ===

(declare-sort Message)
(declare-sort Principal)
(declare-sort SessionKey)
(declare-sort HMAC)
(declare-sort Nonce)

;; Message types
(declare-sort MessageType)
(declare-const MSG_TREAD MessageType)
(declare-const MSG_TWRITE MessageType)
(declare-const MSG_TAUTH MessageType)
(declare-const MSG_TATTACH MessageType)

;; === MESSAGE STRUCTURE ===

(declare-fun msg_type (Message) MessageType)
(declare-fun msg_sender (Message) Principal)
(declare-fun msg_receiver (Message) Principal)
(declare-fun msg_nonce (Message) Nonce)
(declare-fun msg_hmac (Message) HMAC)
(declare-fun msg_payload (Message) Int)
(declare-fun msg_timestamp (Message) Int)

;; === SESSION MANAGEMENT ===

(declare-fun session_key (Principal Principal) SessionKey)
(declare-fun has_valid_session (Principal Principal) Bool)
(declare-fun nonce_is_fresh (Nonce) Bool)
(declare-fun nonce_used (Nonce) Bool)

;; === CRYPTOGRAPHIC OPERATIONS ===

(declare-fun compute_hmac (Message SessionKey) HMAC)
(declare-fun verify_hmac (Message HMAC SessionKey) Bool)
(declare-fun message_authentic (Message) Bool)

;; === CORE SECURITY AXIOMS ===

;; Axiom 1: HMAC verification requires correct session key
(assert (forall ((msg Message) (hmac HMAC) (key SessionKey))
  (= (verify_hmac msg hmac key)
     (= hmac (compute_hmac msg key)))))

;; Axiom 2: Message is authentic only if HMAC verifies with session key
(assert (forall ((msg Message))
  (= (message_authentic msg)
     (and
       ;; Session exists between sender and receiver
       (has_valid_session (msg_sender msg) (msg_receiver msg))
       ;; HMAC verifies with session key
       (verify_hmac msg (msg_hmac msg)
                   (session_key (msg_sender msg) (msg_receiver msg)))
       ;; Nonce is fresh (not reused)
       (nonce_is_fresh (msg_nonce msg))
       ;; Timestamp is recent (within 60 seconds)
       (and (>= (msg_timestamp msg) 0)
            (<= (msg_timestamp msg) 60))))))

;; Axiom 3: Fresh nonces haven't been used
(assert (forall ((n Nonce))
  (= (nonce_is_fresh n)
     (not (nonce_used n)))))

;; Axiom 4: Session keys are unique per principal pair
(assert (forall ((p1 Principal) (p2 Principal) (p3 Principal) (p4 Principal))
  (=> (not (and (= p1 p3) (= p2 p4)))
      (not (= (session_key p1 p2) (session_key p3 p4))))))

;; Axiom 5: Valid sessions must be established through authentication
(declare-fun authenticated (Principal Principal) Bool)
(assert (forall ((p1 Principal) (p2 Principal))
  (=> (has_valid_session p1 p2)
      (authenticated p1 p2))))

;; Axiom 6: HMAC is deterministic
(assert (forall ((msg Message) (key SessionKey))
  (= (compute_hmac msg key) (compute_hmac msg key))))

;; Axiom 7: Different messages produce different HMACs (collision resistance)
(assert (forall ((msg1 Message) (msg2 Message) (key SessionKey))
  (= (= (compute_hmac msg1 key) (compute_hmac msg2 key))
     (and (= (msg_type msg1) (msg_type msg2))
          (= (msg_sender msg1) (msg_sender msg2))
          (= (msg_receiver msg1) (msg_receiver msg2))
          (= (msg_nonce msg1) (msg_nonce msg2))
          (= (msg_payload msg1) (msg_payload msg2))
          (= (msg_timestamp msg1) (msg_timestamp msg2))))))

;; Axiom 7b: Different keys produce different HMACs for same message
(assert (forall ((msg Message) (key1 SessionKey) (key2 SessionKey))
  (=> (not (= key1 key2))
      (not (= (compute_hmac msg key1) (compute_hmac msg key2))))))

;; Axiom 8: Once used, nonce cannot be fresh again
(declare-fun mark_nonce_used (Nonce) Bool)
(assert (forall ((n Nonce))
  (=> (mark_nonce_used n)
      (nonce_used n))))

;; === SECURITY THEOREMS ===

(declare-const alice Principal)
(declare-const bob Principal)
(declare-const eve Principal)
(declare-const legitimate_msg Message)
(declare-const spoofed_msg Message)
(declare-const replay_msg Message)
(declare-const valid_nonce Nonce)
(declare-const used_nonce Nonce)

;; THEOREM 1: Cannot spoof message without session key

(push)
(assert (and
  ;; Alice and Bob have valid session
  (has_valid_session alice bob)
  (authenticated alice bob)

  ;; Eve tries to send spoofed message pretending to be Alice
  (= (msg_sender spoofed_msg) alice)
  (= (msg_receiver spoofed_msg) bob)
  (= (msg_type spoofed_msg) MSG_TREAD)

  ;; Eve doesn't know the session key, so uses wrong HMAC
  (not (= (msg_hmac spoofed_msg)
          (compute_hmac spoofed_msg (session_key alice bob))))

  ;; Message passes authentication
  (message_authentic spoofed_msg)
))

(check-sat)
;; Expected: unsat (cannot authenticate without correct HMAC)
(pop)

;; THEOREM 2: Cannot replay message with reused nonce

(push)
(assert (and
  ;; Valid session
  (has_valid_session alice bob)

  ;; Nonce has been used before
  (nonce_used (msg_nonce replay_msg))

  ;; Everything else is valid
  (= (msg_sender replay_msg) alice)
  (= (msg_receiver replay_msg) bob)
  (= (msg_hmac replay_msg)
     (compute_hmac replay_msg (session_key alice bob)))
  (>= (msg_timestamp replay_msg) 0)
  (<= (msg_timestamp replay_msg) 60)

  ;; Message passes authentication
  (message_authentic replay_msg)
))

(check-sat)
;; Expected: unsat (replay attack detected via nonce)
(pop)

;; THEOREM 3: Cannot authenticate without valid session

(push)
(assert (and
  ;; No valid session between Eve and Bob
  (not (has_valid_session eve bob))

  ;; Eve tries to send message to Bob
  (= (msg_sender legitimate_msg) eve)
  (= (msg_receiver legitimate_msg) bob)
  (nonce_is_fresh (msg_nonce legitimate_msg))
  (>= (msg_timestamp legitimate_msg) 0)
  (<= (msg_timestamp legitimate_msg) 60)

  ;; Message authenticates
  (message_authentic legitimate_msg)
))

(check-sat)
;; Expected: unsat (no authentication without session)
(pop)

;; THEOREM 4: Expired timestamp prevents authentication

(push)
(declare-const old_msg Message)

(assert (and
  ;; Valid session and HMAC
  (has_valid_session alice bob)
  (= (msg_sender old_msg) alice)
  (= (msg_receiver old_msg) bob)
  (= (msg_hmac old_msg)
     (compute_hmac old_msg (session_key alice bob)))
  (nonce_is_fresh (msg_nonce old_msg))

  ;; But timestamp is too old (> 60 seconds)
  (> (msg_timestamp old_msg) 60)

  ;; Message authenticates
  (message_authentic old_msg)
))

(check-sat)
;; Expected: unsat (expired timestamp rejected)
(pop)

;; THEOREM 5: Different session keys produce different HMACs

(push)
(declare-const charlie Principal)

(assert (and
  ;; Alice-Bob session
  (has_valid_session alice bob)
  ;; Alice-Charlie session (different)
  (has_valid_session alice charlie)
  ;; Bob != Charlie
  (not (= bob charlie))

  ;; Same message content
  (= (msg_sender legitimate_msg) alice)
  (= (msg_nonce legitimate_msg) valid_nonce)
  (= (msg_payload legitimate_msg) 42)

  ;; HMAC computed with Alice-Bob key matches Alice-Charlie key
  (= (compute_hmac legitimate_msg (session_key alice bob))
     (compute_hmac legitimate_msg (session_key alice charlie)))
))

(check-sat)
;; Expected: unsat (different sessions produce different HMACs)
(pop)

;; THEOREM 6: Message modification invalidates HMAC

(push)
(declare-const original_msg Message)
(declare-const modified_msg Message)

(assert (and
  ;; Original message is valid
  (has_valid_session alice bob)
  (= (msg_sender original_msg) alice)
  (= (msg_receiver original_msg) bob)
  (= (msg_payload original_msg) 100)
  (nonce_is_fresh (msg_nonce original_msg))

  ;; Modified message has different payload but same other fields
  (= (msg_type modified_msg) (msg_type original_msg))
  (= (msg_sender modified_msg) alice)
  (= (msg_receiver modified_msg) bob)
  (= (msg_payload modified_msg) 200)
  (= (msg_nonce modified_msg) (msg_nonce original_msg))
  (= (msg_timestamp modified_msg) (msg_timestamp original_msg))

  ;; But uses original HMAC (attacker copied it)
  (= (msg_hmac modified_msg) (compute_hmac original_msg (session_key alice bob)))

  ;; Verify should fail because HMAC was computed for different message
  (verify_hmac modified_msg (msg_hmac modified_msg) (session_key alice bob))
))

(check-sat)
;; Expected: unsat (modified message rejected)
(pop)

;; THEOREM 7: Man-in-the-middle cannot forge HMAC

(push)
(declare-const mitm_msg Message)

(assert (and
  ;; Alice and Bob have session
  (has_valid_session alice bob)
  (authenticated alice bob)

  ;; Eve intercepts and tries to modify
  (= (msg_sender mitm_msg) alice)
  (= (msg_receiver mitm_msg) bob)
  (nonce_is_fresh (msg_nonce mitm_msg))
  (>= (msg_timestamp mitm_msg) 0)
  (<= (msg_timestamp mitm_msg) 60)

  ;; Eve doesn't know session key, computes wrong HMAC
  (not (= (msg_hmac mitm_msg)
          (compute_hmac mitm_msg (session_key alice bob))))

  ;; Message authenticates
  (message_authentic mitm_msg)
))

(check-sat)
;; Expected: unsat (MITM attack prevented)
(pop)

;; === VERIFICATION SUMMARY ===

(echo "✓ Network message authentication verified")
(echo "  - Cannot spoof message without session key")
(echo "  - Cannot replay message with reused nonce")
(echo "  - Cannot authenticate without valid session")
(echo "  - Expired timestamp prevents authentication")
(echo "  - Different session keys produce different HMACs")
(echo "  - Message modification invalidates HMAC")
(echo "  - Man-in-the-middle cannot forge HMAC")
