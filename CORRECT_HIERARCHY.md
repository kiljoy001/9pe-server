# The Correct File Hierarchy

## The Three Tiers of Files

### 1. Normal Files
**Operations**: `read`, `write`, `execute`
- Traditional files - store and retrieve data
- Can be executed if they're scripts/binaries
- NO inherent computation
- What you know and expect

```bash
echo "hello" > /tmp/file.txt       # Write
cat /tmp/file.txt                   # Read (returns: "hello")
chmod +x script.sh && ./script.sh  # Execute
```

### 2. Synthetic Files
**Operations**: `read`, `write`, `execute`, **`compute`**
- Generated/computed content
- Write = set parameters
- Read = trigger computation
- Stateful between operations

```bash
# Synthetic file that computes
echo "5" > /synthetic/square       # Set input
cat /synthetic/square               # Compute: returns "25"

# The computation happens on read
echo "10" > /synthetic/square      # Change input
cat /synthetic/square               # Recompute: returns "100"
```

### 3. Translators
**Operations**: `read`, `write`, `execute`, **`compute`**, **`compose`**
- Everything synthetic files can do PLUS
- Can be chained together
- Transform data streams
- Create new translators via composition

```bash
# Individual translators
echo "hello" > /trans/uppercase
cat /trans/uppercase                # Returns: "HELLO"

# Composition - the unique power of translators
echo "uppercase|base64" > /trans/compose/pipeline
echo "hello" > /trans/compose/pipeline
cat /trans/compose/pipeline         # Returns: "SEVMTE8="

# Create new translator from composition
ln -s /trans/compose/pipeline /trans/my_encoder
```

## The Key Distinctions

### Normal Files Are Dumb
- Just bytes on disk
- No computation
- What Plan 9 had

### Synthetic Files Are Smart
- Compute on demand
- Examples:
  - `/proc/cpuinfo` - generates fresh CPU data
  - `/sys/random` - produces random bytes
  - `/ai/model` - runs inference

### Translators Are Composable
- Not just smart, but COMBINABLE
- The real magic of the system
- Examples:
  - `/trans/json2xml` - data format translator
  - `/trans/encrypt` - security translator
  - `/trans/compress` - compression translator

## Why This Hierarchy Matters

### For CLI Wizards

**Normal files**: Your familiar playground
```bash
cat data.txt | grep "error" | wc -l
```

**Synthetic files**: Your new superpowers
```bash
# Don't store sensor data, compute it fresh
cat /synthetic/sensor/temperature

# Don't cache API responses, fetch live
cat /synthetic/api/weather
```

**Translators**: Your composition toolkit
```bash
# Build complex pipelines from simple parts
echo "parse_json|extract_field:price|threshold:1000" > /trans/compose/price_alert

# Now use it anywhere
cat api_response.json | /trans/compose/price_alert
```

## The Pebbling Connection

This hierarchy maps perfectly to computation graphs:

1. **Normal files** = Leaf nodes (data)
2. **Synthetic files** = Compute nodes (functions)
3. **Translators** = Graph edges (transformations)

Pebbling can optimize the entire graph:
- Know when to compute synthetic files
- Cache translator results
- Minimize memory usage

## User Creation Patterns

### Creating Synthetic Files
```bash
# Users drop WASM that implements compute()
cp sensor.wasm /synthetic/install/

# Now it's a synthetic file
cat /synthetic/sensor  # Calls compute()
```

### Creating Translators
```bash
# Users drop WASM that implements compute() AND compose()
cp converter.wasm /trans/install/

# Now it's a translator
echo "data" | /trans/converter  # Compute
echo "converter|uppercase" > /trans/compose/new  # Compose!
```

## The Beauty of Constraints

By limiting computation to synthetic files and translators, we:

1. **Keep normal files simple** - No surprises
2. **Make computation explicit** - You know what computes
3. **Enable optimization** - System knows the computation graph
4. **Maintain compatibility** - Normal files work everywhere

This isn't "everything computes" - it's "computation where it makes sense."

## The CLI Wizard's Mental Model

```bash
# Normal file = storage
echo "data" > /tmp/file

# Synthetic file = function with state
echo "input" > /synthetic/function
cat /synthetic/function  # Computes f(input)

# Translator = composable function
cat data | /trans/t1 | /trans/t2 | /trans/t3

# The magic: translators can become synthetic files
ln -s "/trans/t1|t2|t3" /synthetic/pipeline
echo "input" > /synthetic/pipeline
cat /synthetic/pipeline  # Runs through entire pipeline
```

## Summary

- **Normal files**: Just files (read, write, execute)
- **Synthetic files**: Computed files (+ compute)
- **Translators**: Composable computed files (+ compose)

This is the sweet spot:
- Not too simple (everything is static)
- Not too complex (everything computes)
- Just right (computation where valuable)

**The hierarchy is the innovation.**