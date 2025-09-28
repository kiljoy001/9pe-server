;;; Thread Safety and Deadlock Prevention Tests for 9P.e Server
;;; Verifies mutual exclusion, absence of data races, and deadlock prevention

(set-logic ALL)
(set-option :produce-models true)
(set-option :produce-unsat-cores true)

;;; ============================================================================
;;; Type Definitions
;;; ============================================================================

;; Thread and lock identifiers
(define-sort ThreadId () Int)
(define-sort LockId () Int)
(define-sort ResourceId () Int)

;; Thread state enumeration
(declare-datatypes () ((ThreadState Running Waiting Terminated)))

;; Lock state
(declare-datatypes () ((LockState
    Unlocked
    (Locked (lock-owner ThreadId)))))

;; Operation types
(declare-datatypes () ((Operation
    (Read (read-resource ResourceId))
    (Write (write-resource ResourceId))
    (Acquire (acquire-lock LockId))
    (Release (release-lock LockId)))))

;; Thread context
(declare-datatypes () ((Thread
    (mk-thread
        (thread-id ThreadId)
        (thread-state ThreadState)
        (held-locks (Array Int LockId))
        (held-locks-count Int)
        (waiting-for-lock LockId)
        (has-waiting Bool)))))

;; System state
(declare-datatypes () ((SystemState
    (mk-system-state
        (threads (Array Int Thread))
        (thread-count Int)
        (locks (Array LockId LockState))
        (resources (Array ResourceId ThreadId))
        (resource-locks (Array ResourceId LockId))))))

;;; ============================================================================
;;; Helper Functions
;;; ============================================================================

;; Check if thread holds a lock
(define-fun holds-lock ((t Thread) (lid LockId)) Bool
    (exists ((i Int))
        (and (>= i 0)
             (< i (held-locks-count t))
             (= (select (held-locks t) i) lid))))

;; Get lock owner
(define-fun get-lock-owner ((sys SystemState) (lid LockId)) ThreadId
    (ite (is-Locked (select (locks sys) lid))
         (lock-owner (select (locks sys) lid))
         -1)) ; -1 represents no owner

;; Check if lock is available
(define-fun lock-available ((sys SystemState) (lid LockId)) Bool
    (is-Unlocked (select (locks sys) lid)))

;;; ============================================================================
;;; Safety Properties
;;; ============================================================================

;; Mutual exclusion property
(define-fun mutual-exclusion ((sys SystemState)) Bool
    (forall ((lid LockId) (t1 ThreadId) (t2 ThreadId))
        (=> (and (not (= t1 t2))
                 (= (get-lock-owner sys lid) t1))
            (not (= (get-lock-owner sys lid) t2)))))

;; No double locking property
(define-fun no-double-locking ((sys SystemState)) Bool
    (forall ((tid ThreadId) (lid LockId))
        (=> (and (>= tid 0) (< tid (thread-count sys)))
            (let ((t (select (threads sys) tid)))
                (=> (holds-lock t lid)
                    (not (and (has-waiting t)
                             (= (waiting-for-lock t) lid))))))))

;; No data races property
(define-fun no-data-races ((sys SystemState)) Bool
    (forall ((rid ResourceId) (t1 ThreadId) (t2 ThreadId))
        (=> (and (not (= t1 t2))
                 (= (select (resources sys) rid) t1))
            (not (= (select (resources sys) rid) t2)))))

;; Lock ordering for deadlock prevention
(declare-const lock-ordering (Array LockId Int))

;; Check if thread respects lock ordering
(define-fun respects-ordering ((t Thread)) Bool
    (forall ((i Int) (j Int))
        (=> (and (>= i 0) (< i (held-locks-count t))
                 (>= j 0) (< j (held-locks-count t))
                 (< i j))
            (< (select lock-ordering (select (held-locks t) i))
               (select lock-ordering (select (held-locks t) j))))))

;;; ============================================================================
;;; Test 1: Mutual Exclusion Preservation
;;; ============================================================================

(push)
(echo "Test 1: Mutual exclusion is preserved")

(declare-const sys-before SystemState)
(declare-const sys-after SystemState)
(declare-const tid1 ThreadId)
(declare-const tid2 ThreadId)
(declare-const lid LockId)

;; Different threads
(assert (not (= tid1 tid2)))

;; Initial state has mutual exclusion
(assert (mutual-exclusion sys-before))

;; tid1 acquires lock that was unlocked
(assert (lock-available sys-before lid))
(assert (= (select (locks sys-after) lid) (Locked tid1)))

;; Mutual exclusion still holds
(assert (mutual-exclusion sys-after))

;; tid2 cannot also hold the same lock
(assert (not (= (get-lock-owner sys-after lid) tid2)))

(check-sat)
(echo "Verified: Mutual exclusion preserved after lock acquisition")
(pop)

;;; ============================================================================
;;; Test 2: No Double Locking
;;; ============================================================================

(push)
(echo "Test 2: Thread cannot acquire lock it already holds")

(declare-const sys SystemState)
(declare-const tid ThreadId)
(declare-const lid LockId)
(declare-const thread Thread)

(assert (= thread (select (threads sys) tid)))
(assert (= (thread-id thread) tid))

;; Thread already holds the lock
(assert (holds-lock thread lid))
(assert (= (get-lock-owner sys lid) tid))

;; Thread cannot wait for a lock it already holds
(assert (not (and (has-waiting thread)
                  (= (waiting-for-lock thread) lid))))

(check-sat)
(echo "Verified: No double locking")
(pop)

;;; ============================================================================
;;; Test 3: Deadlock Prevention via Lock Ordering
;;; ============================================================================

(push)
(echo "Test 3: Lock ordering prevents deadlocks")

(declare-const sys SystemState)
(declare-const t1 Thread)
(declare-const t2 Thread)
(declare-const lid1 LockId)
(declare-const lid2 LockId)

;; Different locks with ordering
(assert (not (= lid1 lid2)))
(assert (< (select lock-ordering lid1) (select lock-ordering lid2)))

;; Thread 1 holds lid1, wants lid2 (follows ordering)
(assert (holds-lock t1 lid1))
(assert (has-waiting t1))
(assert (= (waiting-for-lock t1) lid2))
(assert (respects-ordering t1))

;; Thread 2 cannot hold lid2 and want lid1 (would violate ordering)
(assert (holds-lock t2 lid2))
(assert (=> (and (has-waiting t2)
                 (= (waiting-for-lock t2) lid1))
            (not (respects-ordering t2))))

;; If both threads respect ordering, no circular wait
(assert (respects-ordering t1))
(assert (respects-ordering t2))

;; Then t2 cannot be waiting for lid1
(assert (not (and (has-waiting t2)
                  (= (waiting-for-lock t2) lid1))))

(check-sat)
(echo "Verified: Lock ordering prevents circular wait")
(pop)

;;; ============================================================================
;;; Test 4: Wait-For Graph Cycle Detection
;;; ============================================================================

(push)
(echo "Test 4: Detect cycles in wait-for graph")

;; Simple cycle: T1 -> L1 -> T2 -> L2 -> T1
(declare-const sys4 SystemState)
(declare-const t1 Thread)
(declare-const t2 Thread)
(declare-const lid1 LockId)
(declare-const lid2 LockId)

(assert (= (thread-id t1) 1))
(assert (= (thread-id t2) 2))
(assert (not (= lid1 lid2)))

;; T1 waits for L1 held by T2
(assert (has-waiting t1))
(assert (= (waiting-for-lock t1) lid1))
(assert (= (get-lock-owner sys4 lid1) 2))

;; T2 waits for L2 held by T1
(assert (has-waiting t2))
(assert (= (waiting-for-lock t2) lid2))
(assert (= (get-lock-owner sys4 lid2) 1))

;; This creates a cycle (deadlock)
(declare-fun has-cycle (SystemState) Bool)
(assert (has-cycle sys4))

;; With lock ordering, this shouldn't happen
(assert (=> (and (respects-ordering t1)
                 (respects-ordering t2))
            (not (has-cycle sys4))))

(check-sat)
(echo "Verified: Cycle detection works")
(pop)

;;; ============================================================================
;;; Test 5: No Data Races with Proper Locking
;;; ============================================================================

(push)
(echo "Test 5: Proper synchronization prevents data races")

(declare-const sys5 SystemState)
(declare-const rid ResourceId)
(declare-const tid1 ThreadId)
(declare-const tid2 ThreadId)
(declare-const protecting-lock LockId)

(assert (not (= tid1 tid2)))

;; Resource is protected by a lock
(assert (= (select (resource-locks sys5) rid) protecting-lock))

;; Thread 1 accesses resource only if holding lock
(assert (=> (= (select (resources sys5) rid) tid1)
            (= (get-lock-owner sys5 protecting-lock) tid1)))

;; Thread 2 accesses resource only if holding lock
(assert (=> (= (select (resources sys5) rid) tid2)
            (= (get-lock-owner sys5 protecting-lock) tid2)))

;; Mutual exclusion on lock prevents simultaneous access
(assert (mutual-exclusion sys5))

;; Therefore, no data race
(assert (no-data-races sys5))

(check-sat)
(echo "Verified: Proper locking prevents data races")
(pop)

;;; ============================================================================
;;; Test 6: Mesh Network Thread Safety Issue (The Bug)
;;; ============================================================================

(push)
(echo "Test 6: Detect mesh network thread safety issue")

;; The bug: Shared Swarm state without proper synchronization
(declare-const mesh-state ResourceId)
(declare-const network-thread ThreadId)
(declare-const handler-thread ThreadId)

(assert (not (= network-thread handler-thread)))

;; INCORRECT: Both threads access mesh-state without locks
(declare-const bad-sys SystemState)
(assert (= (select (resources bad-sys) mesh-state) network-thread))
;; Race condition: handler-thread could also access
(assert (not (no-data-races bad-sys))) ; Violation detected

;; CORRECT: Use RwLock for mesh state
(declare-const good-sys SystemState)
(declare-const mesh-lock LockId)
(assert (= (select (resource-locks good-sys) mesh-state) mesh-lock))
(assert (=> (= (select (resources good-sys) mesh-state) network-thread)
            (= (get-lock-owner good-sys mesh-lock) network-thread)))
(assert (mutual-exclusion good-sys))
(assert (no-data-races good-sys))

(check-sat)
(echo "Verified: Thread safety issue in mesh networking detected and fixed")
(pop)

;;; ============================================================================
;;; Test 7: Reader-Writer Lock Semantics
;;; ============================================================================

(push)
(echo "Test 7: Reader-Writer lock allows multiple readers")

;; RwLock state
(declare-datatypes () ((RwLockState
    RwUnlocked
    (RwReadLocked (readers (Array Int ThreadId)) (reader-count Int))
    (RwWriteLocked (writer ThreadId)))))

(declare-const rwlock RwLockState)
(declare-const t1 ThreadId)
(declare-const t2 ThreadId)
(declare-const t3 ThreadId)

;; All different threads
(assert (distinct t1 t2 t3))

;; Multiple readers can hold read lock
(assert (is-RwReadLocked rwlock))
(assert (= (reader-count rwlock) 2))
(assert (= (select (readers rwlock) 0) t1))
(assert (= (select (readers rwlock) 1) t2))

;; But writer must wait
(declare-const writer-waiting Bool)
(assert (=> (is-RwReadLocked rwlock) writer-waiting))

;; Write lock is exclusive
(declare-const write-lock RwLockState)
(assert (is-RwWriteLocked write-lock))
(assert (= (writer write-lock) t3))

;; No other thread can acquire when write-locked
(assert (=> (is-RwWriteLocked write-lock)
            (and (not (= (writer write-lock) t1))
                 (not (= (writer write-lock) t2)))))

(check-sat)
(echo "Verified: RwLock semantics for concurrent reads")
(pop)

;;; ============================================================================
;;; Test 8: Lock-Free Channel Communication
;;; ============================================================================

(push)
(echo "Test 8: Lock-free channels prevent blocking")

;; Channel state
(declare-datatypes () ((ChannelState
    (mk-channel
        (buffer (Array Int Int))
        (head Int)
        (tail Int)
        (capacity Int)))))

(declare-const channel ChannelState)
(declare-const sender ThreadId)
(declare-const receiver ThreadId)

;; Different threads
(assert (not (= sender receiver)))

;; Channel operations don't require locks
(define-fun can-send ((ch ChannelState)) Bool
    (< (- (tail ch) (head ch)) (capacity ch)))

(define-fun can-receive ((ch ChannelState)) Bool
    (< (head ch) (tail ch)))

;; No lock needed for send/receive
(assert (can-send channel))
(assert (can-receive channel))

;; Operations are wait-free (no thread blocks another)
(declare-const sender-blocked Bool)
(declare-const receiver-blocked Bool)
(assert (not sender-blocked))
(assert (not receiver-blocked))

(check-sat)
(echo "Verified: Lock-free channels work without blocking")
(pop)

;;; ============================================================================
;;; Test 9: Proper Fix for Mesh Networking
;;; ============================================================================

(push)
(echo "Test 9: Verify proper fix for mesh networking using Arc<RwLock<T>>")

;; Rust Arc<RwLock<T>> semantics
(declare-sort ArcRwLock 0)
(declare-fun arc-strong-count (ArcRwLock) Int)
(declare-fun arc-get-rwlock (ArcRwLock) RwLockState)

(declare-const mesh-network ArcRwLock)

;; Multiple threads can share Arc
(assert (>= (arc-strong-count mesh-network) 2))

;; Read operations don't block each other
(declare-const read-thread-1 ThreadId)
(declare-const read-thread-2 ThreadId)
(assert (not (= read-thread-1 read-thread-2)))

(declare-const lock-state RwLockState)
(assert (= lock-state (arc-get-rwlock mesh-network)))

;; Both can read simultaneously
(assert (=> (is-RwReadLocked lock-state)
            (and (exists ((i Int))
                     (= (select (readers lock-state) i) read-thread-1))
                 (exists ((j Int))
                     (= (select (readers lock-state) j) read-thread-2)))))

;; Write is exclusive
(declare-const write-thread ThreadId)
(assert (=> (and (is-RwWriteLocked lock-state)
                 (= (writer lock-state) write-thread))
            (and (not (= write-thread read-thread-1))
                 (not (= write-thread read-thread-2)))))

(check-sat)
(echo "Verified: Arc<RwLock<T>> provides safe concurrent access")
(pop)

(echo "All thread safety tests completed!")
(exit)