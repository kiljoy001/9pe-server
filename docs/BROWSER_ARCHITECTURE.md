# Lux9 "App Browser" Architecture

## Core Philosophy
The Lux9 Browser is not a monolithic application that renders HTML. It is a **kernel service** (`9pe-server`) that translates decentralized protocols and application logic into a unified **9P Filesystem**.

**"The Filesystem is the DOM"**

Instead of parsing HTML/CSS/JS, the client (9front) interacts with a file hierarchy. 
- **Reading a file** = Viewing content.
- **Writing to a file** = User input / API call.
- **Walking a directory** = Navigation.

## 1. Protocol Gateways
The server acts as a universal gateway, translating heterogenous network protocols into standard 9P directories.

### A. Gemini (`/n/gemini`)
*   **Role**: Retrieval of lightweight, semantic documents.
*   **Mapping**:
    *   `gemini://example.com/` -> `/n/gemini/example.com/`
    *   `text/gemini` content -> Markdown-like files or simple text.
    *   Links -> Subdirectories or `.lnk` files.
*   **Status**: `src/translators/gemini.rs` exists (Client Impl).
    *   *Roadmap*: Wrap in `Translator` trait to allow `mount -c /srv/gemini /n/gemini`.

### B. Hypercore (`/n/hyper`)
*   **Role**: fast, mutable, P2P data streams (The "Data Layer").
*   **Mapping**:
    *   `hyper://<key>/` -> `/n/hyper/<key>/`
    *   Feeds appear as seekable files or append-only logs.
*   **Use Case**: Decentralized social/chat apps, distributed file sharing.
*   **Status**: `src/translators/hypercore.rs` exists.

## 2. Application Logic (WASM)
*   **Role**: To replace JavaScript using a secure, sandboxed execution environment.
*   **Mechanism**:
    *   Applications are **WASM Modules** loaded into the server.
    *   They export a **9P Server Interface**.
    *   The Server routes 9P messages for a specific path (e.g., `/srv/app/myapp`) directly to the WASM module's `handle_9p_message` function.
*   **Capabilities**:
    *   **State**: The WASM module maintains the "DOM" in its own memory.
    *   **Input**: Writing to `/srv/app/myapp/button` calls WASM, which updates state and changes content of `/srv/app/myapp/label`.
*   **Status**: `src/wasm/` exists with basic 9P hooks.

## 3. UI Protocol (The "Something Else")
Since we are "not doing HTML", the UI is represented as files.
*   **Simple**: `.txt` files for text, images for bitmaps.
*   **Interactive**:
    *   `ctl` files for sending commands (`echo "scroll 10" > ctl`).
    *   Synthetic files representing UI widgets (`button`, `input`).
*   **Advanced**:
    *   Integration with 9front's `rio` windowing system potentially via `/dev/draw` virtualization (future work).
    *   Server-side rendering (via SYCL) to `.bit` files.

## Architecture Diagram

```mermaid
graph TD
    User[9front Client] -->|9P Protocol| Server[9pe-server]
    
    subgraph "Lux9 Browser Kernel"
        Server -->|Route| VFS[Virtual FS /srv]
        
        VFS -->|Mount| Gem[Gemini Translator]
        VFS -->|Mount| Hyp[Hypercore Translator]
        VFS -->|Mount| Wasm[WASM App Container]
        
        Gem -->|TCP/TLS| GeminiNet[Gemini Space]
        Hyp -->|P2P| HyperNet[Hypercore Swarm]
        
        Wasm -.->|Logic| AppState[App State (RAM)]
    end
```

## Implementation Plan
1.  **Formalize Translator Interface**: Ensure `Gemini`, `Hypercore`, and `WASM` implement a common `Translator` trait.
2.  **Wire Translators**: Update `settrans.rs` to allow mounting these specific translators.
3.  **Enhance WASM Host**: Connect `9p.read`/`9p.write` host functions to allow WASM apps to access the broader 9P namespace (e.g., a WASM app reading from `/n/gemini`).
