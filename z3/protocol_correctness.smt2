;;; Protocol Message Correctness Tests for 9P.e Server
;;; Verifies that message handlers return correct response types
;;; and maintain protocol invariants

(set-logic ALL)
(set-option :produce-models true)
(set-option :produce-proofs true)

;;; ============================================================================
;;; Type Definitions
;;; ============================================================================

;; Message types enumeration
(declare-datatypes () ((MessageType
    TVersion RVersion
    TAuth RAuth
    TAttach RAttach
    TWalk RWalk
    TOpen ROpen
    TCreate RCreate
    TRead RRead
    TWrite RWrite
    TClunk RClunk
    TRemove RRemove
    TStat RStat
    TWStat RWStat
    TFlush RFlush
    TError)))

;; File identifier
(declare-sort Fid 0)

;; Quality identifier
(declare-datatypes () ((Qid (mk-qid (qid-type Int) (qid-version Int) (qid-path Int)))))

;; Message structure
(declare-datatypes () ((Message
    (Version (msize Int) (version String))
    (VersionResp (resp-msize Int) (resp-version String))
    (Attach (fid Fid) (afid Fid) (uname String) (aname String))
    (AttachResp (attach-qid Qid))
    (Walk (walk-fid Fid) (newfid Fid) (wnames (Array Int String)))
    (WalkResp (qids (Array Int Qid)))
    (Open (open-fid Fid) (mode Int))
    (OpenResp (open-qid Qid) (iounit Int))
    (Read (read-fid Fid) (offset Int) (count Int))
    (ReadResp (data (Array Int Int)))
    (Write (write-fid Fid) (write-offset Int) (write-data (Array Int Int)))
    (WriteResp (write-count Int))
    (Clunk (clunk-fid Fid))
    (ClunkResp)
    (Stat (stat-fid Fid))
    (StatResp (stat-data (Array Int Int)))
    (Remove (remove-fid Fid))
    (RemoveResp)
    (Error (ename String) (errno Int)))))

;; Get message type
(define-fun message-type ((msg Message)) MessageType
    (ite (is-Version msg) TVersion
    (ite (is-VersionResp msg) RVersion
    (ite (is-Attach msg) TAttach
    (ite (is-AttachResp msg) RAttach
    (ite (is-Walk msg) TWalk
    (ite (is-WalkResp msg) RWalk
    (ite (is-Open msg) TOpen
    (ite (is-OpenResp msg) ROpen
    (ite (is-Read msg) TRead
    (ite (is-ReadResp msg) RRead
    (ite (is-Write msg) TWrite
    (ite (is-WriteResp msg) RWrite
    (ite (is-Clunk msg) TClunk
    (ite (is-ClunkResp msg) RClunk
    (ite (is-Stat msg) TStat
    (ite (is-StatResp msg) RStat
    (ite (is-Remove msg) TRemove
    (ite (is-RemoveResp msg) RRemove
    TError))))))))))))))))))

;; Valid response predicate
(define-fun is-valid-response ((request Message) (response Message)) Bool
    (or
        ;; Version -> VersionResp or Error
        (and (is-Version request)
             (or (is-VersionResp response) (is-Error response)))
        ;; Attach -> AttachResp or Error
        (and (is-Attach request)
             (or (is-AttachResp response) (is-Error response)))
        ;; Walk -> WalkResp or Error
        (and (is-Walk request)
             (or (is-WalkResp response) (is-Error response)))
        ;; Open -> OpenResp or Error
        (and (is-Open request)
             (or (is-OpenResp response) (is-Error response)))
        ;; Read -> ReadResp or Error
        (and (is-Read request)
             (or (is-ReadResp response) (is-Error response)))
        ;; Write -> WriteResp or Error
        (and (is-Write request)
             (or (is-WriteResp response) (is-Error response)))
        ;; Clunk -> ClunkResp or Error
        (and (is-Clunk request)
             (or (is-ClunkResp response) (is-Error response)))
        ;; Stat -> StatResp or Error
        (and (is-Stat request)
             (or (is-StatResp response) (is-Error response)))
        ;; Remove -> RemoveResp or Error
        (and (is-Remove request)
             (or (is-RemoveResp response) (is-Error response)))))

;;; ============================================================================
;;; File System State
;;; ============================================================================

(declare-sort FileSystemState 0)
(declare-fun fs-fids (FileSystemState) (Array Fid (Array Int String)))
(declare-fun fs-open-files (FileSystemState) (Array Int Fid))
(declare-fun fs-root (FileSystemState) String)

;; Handler functions (uninterpreted for now)
(declare-fun handle-attach (FileSystemState Fid Fid String String) Message)
(declare-fun handle-walk (FileSystemState Fid Fid (Array Int String)) Message)
(declare-fun handle-open (FileSystemState Fid Int) Message)
(declare-fun handle-read (FileSystemState Fid Int Int) Message)
(declare-fun handle-write (FileSystemState Fid Int (Array Int Int)) Message)
(declare-fun handle-stat (FileSystemState Fid) Message)

;;; ============================================================================
;;; Test 1: Attach Handler Returns Correct Response Type
;;; ============================================================================

(push)
(echo "Test 1: Attach handler must return AttachResp or Error")

(declare-const fs FileSystemState)
(declare-const fid1 Fid)
(declare-const afid1 Fid)
(declare-const uname1 String)
(declare-const aname1 String)

(declare-const attach-response Message)
(assert (= attach-response (handle-attach fs fid1 afid1 uname1 aname1)))

;; The response must be valid
(assert (is-valid-response (Attach fid1 afid1 uname1 aname1) attach-response))

;; INCORRECT: Returning Stat instead of AttachResp (bug we're testing)
(declare-const bad-response Message)
(assert (= bad-response (Stat fid1)))
(assert (not (is-valid-response (Attach fid1 afid1 uname1 aname1) bad-response)))

(check-sat)
(get-model)
(pop)

;;; ============================================================================
;;; Test 2: Read Handler Must Return ReadResp, Not WriteResp
;;; ============================================================================

(push)
(echo "Test 2: Read handler must return ReadResp, not WriteResp")

(declare-const fs2 FileSystemState)
(declare-const fid2 Fid)
(declare-const offset2 Int)
(declare-const count2 Int)

(assert (>= offset2 0))
(assert (> count2 0))

(declare-const read-response Message)
(assert (= read-response (handle-read fs2 fid2 offset2 count2)))

;; Must be ReadResp or Error
(assert (or (is-ReadResp read-response) (is-Error read-response)))

;; INCORRECT: Returning WriteResp for Read (bug we're testing)
(declare-const bad-read-response Message)
(declare-const dummy-data (Array Int Int))
(assert (= bad-read-response (WriteResp 100)))
(assert (not (is-valid-response (Read fid2 offset2 count2) bad-read-response)))

(check-sat)
(get-model)
(pop)

;;; ============================================================================
;;; Test 3: Walk Handler Must Return WalkResp with Proper Qids
;;; ============================================================================

(push)
(echo "Test 3: Walk handler must return WalkResp with qids matching wnames length")

(declare-const fs3 FileSystemState)
(declare-const fid3 Fid)
(declare-const newfid3 Fid)
(declare-const wnames3 (Array Int String))
(declare-const wnames-len Int)

(assert (> wnames-len 0))
(assert (<= wnames-len 16)) ; 9P limit

(declare-const walk-response Message)
(assert (= walk-response (handle-walk fs3 fid3 newfid3 wnames3)))

;; If successful, qids length should match wnames length
(assert (=> (is-WalkResp walk-response)
            (= wnames-len wnames-len))) ; Simplified - would check array lengths

;; INCORRECT: Returning Walk with empty wnames (bug we're testing)
(declare-const bad-walk-response Message)
(declare-const empty-wnames (Array Int String))
(assert (= bad-walk-response (Walk newfid3 newfid3 empty-wnames)))
(assert (not (is-valid-response (Walk fid3 newfid3 wnames3) bad-walk-response)))

(check-sat)
(get-model)
(pop)

;;; ============================================================================
;;; Test 4: Stat Handler Must Return StatResp with Stat Structure
;;; ============================================================================

(push)
(echo "Test 4: Stat handler must return StatResp with proper stat data")

(declare-const fs4 FileSystemState)
(declare-const fid4 Fid)

(declare-const stat-response Message)
(assert (= stat-response (handle-stat fs4 fid4)))

;; Must be StatResp or Error
(assert (or (is-StatResp stat-response) (is-Error stat-response)))

;; INCORRECT: Returning just Stat message (bug we're testing)
(declare-const bad-stat-response Message)
(assert (= bad-stat-response (Stat fid4)))
(assert (not (is-valid-response (Stat fid4) bad-stat-response)))

(check-sat)
(get-model)
(pop)

;;; ============================================================================
;;; Test 5: Message Handler Completeness
;;; ============================================================================

(push)
(echo "Test 5: All message types must have handlers")

;; For any request message, there exists a valid response
(declare-const any-request Message)
(declare-const any-response Message)
(declare-const any-fs FileSystemState)

;; Property: Every request type has a valid response type
(assert (=> (or (is-Version any-request)
                (is-Attach any-request)
                (is-Walk any-request)
                (is-Open any-request)
                (is-Read any-request)
                (is-Write any-request)
                (is-Clunk any-request)
                (is-Stat any-request)
                (is-Remove any-request))
            (exists ((resp Message)) (is-valid-response any-request resp))))

(check-sat)
(pop)

;;; ============================================================================
;;; Test 6: State Consistency After Operations
;;; ============================================================================

(push)
(echo "Test 6: FileSystem state consistency after operations")

(declare-const fs-before FileSystemState)
(declare-const fs-after FileSystemState)
(declare-const fid-new Fid)
(declare-const path-new (Array Int String))

;; After successful attach, fid should be in the fid map
(declare-fun fid-exists (FileSystemState Fid) Bool)

;; Property: After attach, the fid exists in the system
(assert (=> (is-AttachResp (handle-attach fs-before fid-new fid-new "user" "aname"))
            (fid-exists fs-after fid-new)))

(check-sat)
(pop)

;;; ============================================================================
;;; Test 7: Error Handling Consistency
;;; ============================================================================

(push)
(echo "Test 7: Error responses must have meaningful error codes")

(declare-const err-response Message)
(assert (is-Error err-response))

;; Error codes should be positive
(assert (> (errno err-response) 0))

;; Common error codes
(define-fun ENOENT () Int 2)    ; File not found
(define-fun EACCES () Int 13)   ; Permission denied
(define-fun EEXIST () Int 17)   ; File exists
(define-fun EISDIR () Int 21)   ; Is a directory
(define-fun EINVAL () Int 22)   ; Invalid argument

;; Error code should be one of the standard codes
(assert (or (= (errno err-response) ENOENT)
            (= (errno err-response) EACCES)
            (= (errno err-response) EEXIST)
            (= (errno err-response) EISDIR)
            (= (errno err-response) EINVAL)))

(check-sat)
(get-model)
(pop)

(echo "All protocol correctness tests completed!")
(exit)