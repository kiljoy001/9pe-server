# 9P.e Translator System - Job Submission via Filesystem

## Overview

The 9P.e server uses **translators** instead of CLI commands for job submission. This is the Plan 9 way - everything is a file, and translators transform filesystem operations into actual work.

## How It Works

1. **WASM Translators** are loaded from `~/.9pe/translators/`
2. **Virtual filesystems** are mounted at `/srv/settrans/` (synthetic, no physical files)
3. **Writing to special files** triggers job submission
4. **Reading from result files** retrieves job output

## Directory Structure

```
~/.9pe/
├── translators/          # WASM translator modules
│   ├── llm.wasm          # LLM inference translator
│   ├── gpu.wasm          # GPU compute translator
│   └── consensus.wasm    # Consensus/voting translator
├── settrans/             # Virtual mount points (in-memory only)
│   ├── llm/
│   │   ├── submit        # Write job here
│   │   └── result        # Read result here
│   └── gpu/
│       ├── submit
│       └── result
└── n/                    # Auto-mounted remote servers
    ├── machine1_port_5640/
    └── machine2_port_5640/
```

## Example: LLM Job Submission

### Traditional Way (NOT USED)
```bash
# This doesn't exist in 9pe-server:
ninep-server submit-job --type llm --prompt "Hello"
```

### The 9P.e Way (Via Filesystem)
```bash
# 1. Mount the 9P.e server
mount -t 9p localhost:5640 /mnt/9pe

# 2. Submit job by writing to translator
cat > /mnt/9pe/srv/settrans/llm/submit <<EOF
{
  "prompt": "What is 2+2?",
  "max_tokens": 100,
  "temperature": 0.7
}
EOF

# 3. Read result
cat /mnt/9pe/srv/settrans/llm/result
```

### Via WASM Translator API

Translators expose these operations:

```rust
// Translator receives filesystem operations
fn handle_write(path: &str, data: &[u8]) -> Result<()> {
    if path.ends_with("submit") {
        let request: LLMRequest = serde_json::from_slice(data)?;
        let job_id = submit_to_consensus_layer(request)?;
        store_job_id(job_id);
    }
    Ok(())
}

fn handle_read(path: &str) -> Result<Vec<u8>> {
    if path.ends_with("result") {
        let job_id = get_stored_job_id()?;
        let result = poll_consensus_layer(job_id)?;
        Ok(serde_json::to_vec(&result)?)
    }
    // ...
}
```

## Current Status

**✅ Working:**
- Translator registry at `~/.9pe/translators/`
- Settrans virtual filesystem
- Auto-mount for remote servers
- Metrics endpoint: `http://localhost:9090/metrics`

**🚧 In Progress:**
- Example WASM translators (need to be compiled and placed in `~/.9pe/translators/`)
- Peer mesh discovery (servers see each other via config but mesh networking stub)

**📝 Next Steps:**

1. **Create Example Translator:**
   ```bash
   # Compile a simple LLM translator to WASM
   cd translators/llm
   cargo build --target wasm32-wasi --release
   cp target/wasm32-wasi/release/llm_translator.wasm ~/.9pe/translators/
   ```

2. **Test Job Submission:**
   ```bash
   # Server will auto-load the translator
   echo '{"prompt": "test"}' > ~/.9pe/settrans/llm/submit
   cat ~/.9pe/settrans/llm/result
   ```

3. **Distributed Jobs:**
   - Jobs submitted to local translator
   - Consensus layer distributes across peers
   - Results aggregated from multiple nodes

## Why This Design?

**Plan 9 Philosophy:**
> "Everything is a file, and files are on servers."

Benefits:
- ✅ No CLI needed - just filesystem operations
- ✅ Network transparent - remote jobs look like local files
- ✅ Composable - `cat`, `echo`, scripts all work
- ✅ Secure - filesystem permissions control access
- ✅ Language agnostic - any language can read/write files

**Example Use Case:**
```bash
# Distributed LLM inference across grid
for prompt in prompts/*.txt; do
  cat $prompt > ~/.9pe/settrans/llm/submit &
done
wait
cat ~/.9pe/settrans/llm/result
```

The grid automatically:
1. Discovers available nodes (Machine 1 has Intel Arc, Machine 2 has Xeon)
2. Distributes work based on capabilities
3. Routes LLM jobs to Machine 1 (has llama.cpp)
4. Routes CPU jobs to Machine 2
5. Aggregates results

## Metrics

Check server health:
```bash
curl http://localhost:9090/metrics
curl http://[201:2bd3:1946:41ea:d21a:a129:2b2f:c3a6]:9090/metrics
```

Output:
```
# 9P.e Metrics
ninep_server_running 1
ninep_connections_total 0
```
