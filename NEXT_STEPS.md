# Next Steps: Making 9P.e Actually Work

## What We Just Proved ✅

Running `cargo test --test minimal_quic_test` shows:
- ✅ QUIC infrastructure compiles
- ✅ Server and client can be created
- ✅ Messages serialize/deserialize
- ✅ DoS protection works
- ✅ Certificates generate properly

**All 5 tests pass in 2.10 seconds.**

## The Gap 🔴

Your code has all the pieces, but they're not connected:
- You have `QuicServer::new()` ✅
- You have `QuicClient::connect()` ✅
- You have message types ✅
- You **don't have** the server loop that ties it together ❌

## The Fix (One Weekend of Work)

### Step 1: Implement Server Message Loop

Edit `src/transport.rs`, in the `QuicServer` impl:

```rust
pub async fn run(&self) -> Result<(), ProtocolError> {
    loop {
        // Accept incoming QUIC connection
        let Some(incoming) = self.endpoint.accept().await else {
            break;
        };

        let rate_limiter = Arc::clone(&self.rate_limiter);

        tokio::spawn(async move {
            // Establish connection
            let connection = incoming.await?;

            // Handle streams
            while let Ok((send, recv)) = connection.accept_bi().await {
                tokio::spawn(async move {
                    let mut session = Session { send, recv, ... };

                    // Message loop
                    loop {
                        let msg = session.read_message().await?;
                        let response = handle_message(msg).await?;
                        session.write_message(&response).await?;
                    }
                });
            }
        });
    }
    Ok(())
}
```

**This is what's missing.** The rest exists.

### Step 2: Implement handle_message()

Start with just Version and Attach:

```rust
async fn handle_message(msg: NinePMessage) -> Result<NinePMessage, ProtocolError> {
    match msg {
        NinePMessage::Version { msize, .. } => {
            Ok(NinePMessage::Version {
                msize: msize.min(MAX_MESSAGE_SIZE),
                version: "9P.e-1.0".into()
            })
        }
        NinePMessage::Attach { .. } => {
            // Return root directory qid
            Ok(NinePMessage::Attach { qid: root_qid() })
        }
        _ => Ok(NinePMessage::Error {
            ename: "Not implemented".into()
        })
    }
}
```

### Step 3: Test It

```bash
# Terminal 1: Start server
cargo run --example minimal_server

# Terminal 2: Test with client
echo "test" > /tmp/test.txt
cargo run --example minimal_client

# Expected output:
# ✅ Connected via QUIC
# ✅ Version negotiation succeeded
# ✅ Attached to root
```

### Step 4: Add File Operations

Implement Walk → Open → Read → Write (3-4 hours)

### Step 5: Test with Real Files

```bash
cargo run --release --example minimal_server

# In another terminal:
cargo run --example minimal_client -- ls /
cargo run --example minimal_client -- cat /tmp/test.txt
cargo run --example minimal_client -- echo "hello" > /tmp/new.txt
```

## Timeline

- **Day 1 (4 hours)**: Server message loop + Version/Attach
- **Day 2 (4 hours)**: Walk/Open/Read operations
- **Day 3 (4 hours)**: Write/Stat/Clunk operations
- **Day 4 (2 hours)**: Integration testing
- **Day 5 (2 hours)**: FUSE mount testing

**Total**: ~16 hours = 1 long weekend

## What NOT to Do

❌ Don't add GHOSTDAG yet
❌ Don't add translators yet
❌ Don't add GPU support yet
❌ Don't add mesh networking yet

These are all valuable, but they assume the core works.

## Success Criteria

You'll know it works when:
```bash
$ cargo run --release --example minimal_server &
$ echo "hello from 9P.e!" > test.txt
$ cargo run --example minimal_client -- cat test.txt
hello from 9P.e!
```

Then, and only then, add the fancy features.

## Reference Files

- **Test**: `tests/minimal_quic_test.rs` - Shows what works
- **Example**: `examples/minimal_server.rs` - Shows what's needed
- **Status**: `TESTING_STATUS.md` - Current state
- **Implementation**: `src/transport.rs` - Where to add the loop

## The Bottom Line

You have:
- 62 formal proofs ✅
- QUIC transport code ✅
- Message types ✅
- Security primitives ✅

You need:
- Server message loop (150 lines of code)
- Message handlers (200 lines of code)

That's it. One focused weekend. Then everything else unlocks.
