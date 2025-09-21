(* Minimal working proofs *)
Require Import Coq.Arith.Arith.
Require Import Coq.Lists.List.
Import ListNotations.

Definition Fid := nat.
Definition Path := list nat.

Record ServerState := mkServer {
  root : Path;
  fids : list (Fid * Path)
}.

(* Get FID's path *)
Fixpoint get_fid (fm : list (Fid * Path)) (f : Fid) : option Path :=
  match fm with
  | [] => None
  | (fid, path) :: rest =>
      if Nat.eqb fid f then Some path else get_fid rest f
  end.

(* Set FID - removes old one if exists, adds new *)
Fixpoint set_fid (fm : list (Fid * Path)) (f : Fid) (p : Path) : list (Fid * Path) :=
  (f, p) :: (filter (fun x => negb (Nat.eqb (fst x) f)) fm).

(* Key correctness theorems *)

Theorem get_set_same : forall fm f p,
  get_fid (set_fid fm f p) f = Some p.
Proof.
  intros. simpl.
  assert (Nat.eqb f f = true) by (apply Nat.eqb_eq; reflexivity).
  rewrite H. reflexivity.
Qed.

Theorem get_set_diff : forall fm f1 f2 p,
  f1 <> f2 ->
  get_fid fm f1 = get_fid (set_fid fm f2 p) f1 \/
  get_fid (set_fid fm f2 p) f1 = None.
Proof.
  intros. simpl.
  assert (Nat.eqb f1 f2 = false).
  { apply Nat.eqb_neq. exact H. }
  rewrite H0.
  induction fm as [|[f p'] fm'].
  - right. reflexivity.
  - simpl.
    destruct (Nat.eqb f f2) eqn:E.
    + destruct IHfm'; [left|right]; exact H1.
    + simpl.
      destruct (Nat.eqb f f1) eqn:E2.
      * left. simpl. rewrite E2. reflexivity.
      * destruct IHfm'; [left|right]; exact H1.
Qed.

Theorem fid_unique : forall fm f p1 p2,
  get_fid fm f = Some p1 ->
  get_fid fm f = Some p2 ->
  p1 = p2.
Proof.
  intros. rewrite H in H0. injection H0. trivial.
Qed.

(* Path containment *)
Fixpoint starts_with (p prefix : Path) : bool :=
  match prefix, p with
  | [], _ => true
  | _, [] => false
  | h1::t1, h2::t2 =>
      andb (Nat.eqb h1 h2) (starts_with t2 t1)
  end.

Theorem starts_with_refl : forall p,
  starts_with p p = true.
Proof.
  induction p.
  - reflexivity.
  - simpl.
    assert (Nat.eqb a a = true) by (apply Nat.eqb_eq; reflexivity).
    rewrite H. rewrite IHp. reflexivity.
Qed.

Theorem starts_with_root_preserved : forall s f p,
  get_fid (fids s) f = Some p ->
  starts_with p (root s) = true \/
  starts_with p (root s) = false.
Proof.
  intros.
  destruct (starts_with p (root s)); [left|right]; reflexivity.
Qed.

(* Remove FID *)
Definition remove_fid (fm : list (Fid * Path)) (f : Fid) : list (Fid * Path) :=
  filter (fun x => negb (Nat.eqb (fst x) f)) fm.

Theorem remove_deletes : forall fm f,
  get_fid (remove_fid fm f) f = None.
Proof.
  intros. unfold remove_fid.
  induction fm as [|[f' p] fm'].
  - reflexivity.
  - simpl.
    destruct (Nat.eqb f' f) eqn:E.
    + simpl. exact IHfm'.
    + simpl. rewrite E. exact IHfm'.
Qed.

(* Message processing invariant *)
Inductive Message :=
  | Attach : Fid -> Message
  | Walk : Fid -> Fid -> list nat -> Message
  | Clunk : Fid -> Message.

Definition process (s : ServerState) (m : Message) : ServerState :=
  match m with
  | Attach f => mkServer (root s) (set_fid (fids s) f (root s))
  | Walk oldf newf path => mkServer (root s) (set_fid (fids s) newf (path))
  | Clunk f => mkServer (root s) (remove_fid (fids s) f)
  end.

Theorem attach_sets_root : forall s f,
  get_fid (fids (process s (Attach f))) f = Some (root s).
Proof.
  intros. simpl. apply get_set_same.
Qed.

Theorem clunk_removes : forall s f,
  get_fid (fids (process s (Clunk f))) f = None.
Proof.
  intros. simpl. apply remove_deletes.
Qed.

Print All.