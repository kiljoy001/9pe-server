# Test Strategy After Legacy Suite Removal

With the legacy integration and property suites disabled, we need a focused, modern testing story that emphasizes breadth via generative techniques and depth where deterministic logic demands it.

## Guiding Principles
- **Property-Based Testing First**: use `proptest`/`quickcheck`-style generators to specify invariants for protocol messages, filesystem operations, and consensus state transitions. Properties should cover both happy paths and adversarial inputs.
- **Fuzzing for I/O Boundaries**: integrate libFuzzer/AFL (via `cargo-fuzz`) against parsers, network framing, and WASM translator inputs. Fuzz harnesses must log minimized repro cases back into the repository.
- **Targeted Unit Tests**: only when a function encodes crisp business logic (e.g., VRAM accounting, job scheduling) should we add table-driven unit tests. Favor invariant/property tests otherwise.
- **SMT/Verification Annotations**: annotate critical algorithms (memory pebbling, consensus rules, translator validation) with specifications amenable to SMT-LIB (`.smt2`) generation. Where feasible, extract verification conditions using tools like Prusti, KLEE, or hand-authored `assert!` macros guarded by `cfg(verification)` to let a solver replace sprawling unit suites.

## Test Backlog (New Suites Required)

### Protocol / Messaging
- Property tests for `NinePMessage` covering all variants (replacing the old suite but aligned with current fields)
- Fuzz harness for 9P message parser and encoder

### Synthetic Filesystem
- Property tests asserting directory invariants (idempotent create/delete, path normalization)
- Unit tests for VRAM allocation ledger and fallback execution paths

### Consensus & Mesh
- Property tests for ghostdag/bounded dag stats (monotonicity, tip invariants)
- Fuzz harness driving QUIC frame handling once transport is stabilized

### WASM Translator & SYCL Jobs
- Property tests to ensure translator registry rejects malformed modules
- Fuzzing of `/gpu/compute/submit` payloads (with sandboxed adapters)
- SMT annotations around VRAM guards and job memory copy lengths

### Authentication / Capability System
- Property tests for token issuance/verification once the new API is finalized

## Tooling Actions
- Add `cargo-fuzz` targets for protocol, translator, and compute payloads
- Add `tests/new/` structure for the upcoming property suites (mirroring the list above)
- Introduce `cfg(verification)` scaffolding in core crates to emit SMT2 obligations as an alternative to deep unit tests

This document is the authoritative plan until new suites land; keep it updated as each class of tests is (re)implemented.

> Note: the legacy consensus/mesh property suites were tied to pre-QUIC APIs. Rather than revive them, we’ll rebuild coverage that matches the new architecture.

> Plan: when rebuilding consensus/mesh coverage, follow a TDD cadence—write the new property/fuzz/unit tests first so they define the required interfaces, then bring the implementation up to match.

> Verification cadence proposal: capture critical specs in Coq first, translate them into executable property tests, implement against those tests, then discharge the properties with SMT (or the originating Coq proofs) for full assurance.
