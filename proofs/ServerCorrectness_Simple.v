(* Simplified Formal Correctness Proofs for 9P.e Server *)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.Bool.Bool.
Import ListNotations.

(* Core types *)
Definition Fid := nat.
Definition Path := list nat.  (* Simplified from string list *)

Record ServerState : Type := mkServer {
  root_path : Path;
  fid_map : list (Fid * Path);
  max_message_size : nat
}.

(* Path containment check *)
Fixpoint path_starts_with (p prefix : Path) : bool :=
  match prefix, p with
  | [], _ => true
  | _, [] => false
  | h1::t1, h2::t2 =>
      if Nat.eqb h1 h2 then path_starts_with t2 t1
      else false
  end.

(* FID operations *)
Definition get_fid (s : ServerState) (fid : Fid) : option Path :=
  match List.find (fun p => Nat.eqb (fst p) fid) (fid_map s) with
  | Some (_, path) => Some path
  | None => None
  end.

Definition set_fid (s : ServerState) (fid : Fid) (path : Path) : ServerState :=
  mkServer (root_path s)
           ((fid, path) :: filter (fun p => negb (Nat.eqb (fst p) fid)) (fid_map s))
           (max_message_size s).

Definition remove_fid (s : ServerState) (fid : Fid) : ServerState :=
  mkServer (root_path s)
           (filter (fun p => negb (Nat.eqb (fst p) fid)) (fid_map s))
           (max_message_size s).

(* ============ KEY THEOREMS ============ *)

(* Theorem 1: FID uniqueness *)
Theorem fid_uniqueness :
  forall s fid path1 path2,
    get_fid s fid = Some path1 ->
    get_fid s fid = Some path2 ->
    path1 = path2.
Proof.
  intros s fid path1 path2 H1 H2.
  rewrite H1 in H2.
  injection H2. trivial.
Qed.

(* Theorem 2: Set FID creates mapping *)
Theorem set_fid_creates :
  forall s fid path,
    get_fid (set_fid s fid path) fid = Some path.
Proof.
  intros s fid path.
  unfold get_fid, set_fid. simpl.
  assert (Nat.eqb fid fid = true).
  { apply Nat.eqb_eq. reflexivity. }
  rewrite H. reflexivity.
Qed.

(* Theorem 3: Remove FID deletes mapping *)
Theorem remove_fid_deletes :
  forall s fid,
    get_fid (remove_fid s fid) fid = None.
Proof.
  intros s fid.
  unfold get_fid, remove_fid. simpl.
  induction (fid_map s).
  - reflexivity.
  - simpl. destruct a as [f p].
    destruct (Nat.eqb fid f) eqn:E.
    + simpl. rewrite E. exact IHl.
    + simpl. rewrite E.
      destruct (Nat.eqb f fid) eqn:E2.
      * apply Nat.eqb_eq in E2.
        rewrite E2 in E.
        rewrite Nat.eqb_refl in E.
        discriminate.
      * exact IHl.
Qed.

(* Theorem 4: Path containment reflexivity *)
Theorem path_starts_with_refl :
  forall p, path_starts_with p p = true.
Proof.
  induction p.
  - reflexivity.
  - simpl. rewrite Nat.eqb_refl. exact IHp.
Qed.

(* Theorem 5: Path containment transitivity *)
Theorem path_starts_with_trans :
  forall p1 p2 p3,
    path_starts_with p1 p2 = true ->
    path_starts_with p2 p3 = true ->
    path_starts_with p1 p3 = true.
Proof.
  induction p3; intros.
  - destruct p2; destruct p1; auto.
    simpl in H0. discriminate.
  - destruct p2.
    + simpl in H0. exact H.
    + destruct p1.
      * reflexivity.
      * simpl in *.
        destruct (Nat.eqb n n0) eqn:E1.
        -- rewrite E1 in H.
           destruct (Nat.eqb n0 a) eqn:E2.
           ++ rewrite E2 in H0.
              apply Nat.eqb_eq in E1.
              apply Nat.eqb_eq in E2.
              subst. rewrite Nat.eqb_refl.
              apply (IHp3 p1 p2 H H0).
           ++ rewrite E2 in H0. discriminate.
        -- rewrite E1 in H. discriminate.
Qed.

(* Theorem 6: Set preserves other FIDs *)
Theorem set_preserves_others :
  forall s fid1 fid2 path new_path,
    fid1 <> fid2 ->
    get_fid s fid1 = Some path ->
    get_fid (set_fid s fid2 new_path) fid1 = Some path.
Proof.
  intros s fid1 fid2 path new_path H_neq H_get.
  unfold get_fid, set_fid. simpl.
  assert (Nat.eqb fid1 fid2 = false).
  { apply Nat.eqb_neq. exact H_neq. }
  rewrite H.
  unfold get_fid in H_get.
  destruct (find (fun p => Nat.eqb (fst p) fid1)
                 (filter (fun p => negb (Nat.eqb (fst p) fid2)) (fid_map s))) eqn:H_find.
  - destruct p0 as [f p].
    assert (find (fun p0 => Nat.eqb (fst p0) fid1) (fid_map s) = Some (f, p)).
    { clear H_get.
      induction (fid_map s).
      - simpl in H_find. discriminate.
      - simpl in *. destruct a as [f' p'].
        destruct (Nat.eqb (fst (f', p')) fid2) eqn:E.
        + simpl in E. rewrite E in H_find. exact IHl.
        + simpl in E. rewrite E in H_find. simpl in H_find.
          destruct (Nat.eqb f' fid1) eqn:E2.
          * simpl in E2. rewrite E2 in H_find.
            injection H_find. intros. subst. rewrite E2. reflexivity.
          * simpl in E2. rewrite E2 in H_find. exact IHl.
    }
    rewrite H0 in H_get. injection H_get. intro. subst. reflexivity.
  -
    (* This case shouldn't happen given H_get, but we need more setup *)
    exfalso.
    clear H.
    unfold get_fid in H_get.
    (* Would need lemma about filter preserving find *)
    admit.
Admitted.

(* Theorem 7: Remove preserves others *)
Theorem remove_preserves_others :
  forall s fid1 fid2 path,
    fid1 <> fid2 ->
    get_fid s fid1 = Some path ->
    get_fid (remove_fid s fid2) fid1 = Some path.
Proof.
  (* Similar to set_preserves_others *)
  admit.
Admitted.

(* Theorem 8: Double set is idempotent on result *)
Theorem set_idempotent :
  forall s fid path1 path2,
    get_fid (set_fid (set_fid s fid path1) fid path2) fid = Some path2.
Proof.
  intros. apply set_fid_creates.
Qed.

(* Theorem 9: State equality decidability *)
Theorem state_eq_dec :
  forall s1 s2 : ServerState, {s1 = s2} + {s1 <> s2}.
Proof.
  intros s1 s2.
  destruct s1 as [r1 f1 m1].
  destruct s2 as [r2 f2 m2].
  destruct (list_eq_dec Nat.eq_dec r1 r2) as [H_root|H_root].
  - destruct (Nat.eq_dec m1 m2) as [H_msize|H_msize].
    + (* Would need decidability for FID map *)
      admit.
    + right. intro H. injection H. intros. contradiction.
  - right. intro H. injection H. intros.
    subst. contradiction.
Admitted.

(* Theorem 10: Path containment is decidable *)
Theorem path_starts_with_dec :
  forall p prefix : Path, {path_starts_with p prefix = true} + {path_starts_with p prefix = false}.
Proof.
  intros p prefix.
  destruct (path_starts_with p prefix) eqn:E.
  - left. reflexivity.
  - right. reflexivity.
Qed.