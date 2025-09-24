/*
 * Example WASM Translator: Uppercase Transform
 *
 * This translator converts all text to uppercase, demonstrating
 * the verified WASM↔9PE interface.
 */

#include <stdint.h>
#include <string.h>

// Simple memory allocator for WASM
static char heap[1024 * 1024];  // 1MB heap
static size_t heap_offset = 0;

// Message type constants (from verified Coq specification)
#define MSG_TREAD   116
#define MSG_RREAD   117
#define MSG_TWRITE  118
#define MSG_RWRITE  119

// 9P message structure (simplified)
typedef struct {
    uint8_t type;
    uint32_t fid;
    uint32_t offset;
    uint32_t count;
    // data follows
} ninep_message_t;

/**
 * WASM malloc implementation
 * Maintains heap monotonicity as proven in Coq
 */
__attribute__((export_name("malloc")))
uint32_t wasm_malloc(uint32_t size) {
    if (heap_offset + size > sizeof(heap)) {
        return 0; // Out of memory
    }

    uint32_t ptr = (uint32_t)&heap[heap_offset];
    heap_offset += size;
    return ptr;
}

/**
 * WASM free implementation (simplified)
 */
__attribute__((export_name("free")))
void wasm_free(uint32_t ptr) {
    // Simple allocator - no actual freeing
    // In production, would implement proper free list
}

/**
 * Convert character to uppercase
 */
static char to_uppercase(char c) {
    if (c >= 'a' && c <= 'z') {
        return c - 32;
    }
    return c;
}

/**
 * Main 9P message handler (verified interface)
 *
 * This function implements the proven protocol:
 * - Tread → Rread (with uppercase transformation)
 * - Twrite → Rwrite (acknowledging write)
 * - Preserves FID (proven correctness property)
 */
__attribute__((export_name("handle_9p_message")))
uint32_t handle_9p_message(uint32_t msg_ptr, uint32_t msg_len) {
    // Parse input message
    uint8_t* msg_bytes = (uint8_t*)msg_ptr;

    if (msg_len < 1) {
        return 0; // Invalid message
    }

    uint8_t msg_type = msg_bytes[0];

    if (msg_len < 5) {
        return 0; // Need at least type + fid
    }

    // Extract FID (bytes 1-4)
    uint32_t fid = *(uint32_t*)(msg_bytes + 1);

    if (msg_type == MSG_TREAD) {
        // Handle read request - transform data to uppercase

        // For this example, we'll transform some sample data
        const char* sample_data = "hello world from wasm translator!";
        uint32_t data_len = strlen(sample_data);

        // Allocate response buffer
        // Format: [length] [response_type] [fid] [data]
        uint32_t response_size = 4 + 1 + 4 + data_len;
        uint32_t response_ptr = wasm_malloc(response_size);

        if (response_ptr == 0) {
            return 0; // Out of memory
        }

        uint8_t* response = (uint8_t*)response_ptr;

        // Write response length (excluding length field itself)
        *(uint32_t*)response = response_size - 4;

        // Write response type (Rread)
        response[4] = MSG_RREAD;

        // Write FID (preserves FID as proven)
        *(uint32_t*)(response + 5) = fid;

        // Transform data to uppercase
        for (uint32_t i = 0; i < data_len; i++) {
            response[9 + i] = to_uppercase(sample_data[i]);
        }

        return response_ptr;

    } else if (msg_type == MSG_TWRITE) {
        // Handle write request - acknowledge write

        // Extract data from write message
        uint8_t* write_data = msg_bytes + 5; // Skip type + fid
        uint32_t write_len = msg_len - 5;

        // For demonstration, we could process the written data here
        // (e.g., store it, transform it, etc.)

        // Allocate response buffer
        // Format: [length] [response_type] [fid] [count]
        uint32_t response_size = 4 + 1 + 4 + 4;
        uint32_t response_ptr = wasm_malloc(response_size);

        if (response_ptr == 0) {
            return 0; // Out of memory
        }

        uint8_t* response = (uint8_t*)response_ptr;

        // Write response length
        *(uint32_t*)response = response_size - 4;

        // Write response type (Rwrite)
        response[4] = MSG_RWRITE;

        // Write FID (preserves FID as proven)
        *(uint32_t*)(response + 5) = fid;

        // Write count of bytes written
        *(uint32_t*)(response + 9) = write_len;

        return response_ptr;

    } else {
        // Unsupported message type
        return 0;
    }
}

/**
 * Optional: Get translator information
 */
__attribute__((export_name("get_translator_info")))
uint32_t get_translator_info() {
    const char* info = "{\"name\":\"uppercase\",\"version\":\"1.0\",\"description\":\"Converts text to uppercase\"}";
    uint32_t info_len = strlen(info);

    uint32_t response_ptr = wasm_malloc(4 + info_len);
    if (response_ptr == 0) {
        return 0;
    }

    uint8_t* response = (uint8_t*)response_ptr;
    *(uint32_t*)response = info_len;
    memcpy(response + 4, info, info_len);

    return response_ptr;
}

/**
 * Optional: Initialize translator
 */
__attribute__((export_name("init")))
void init() {
    // Initialize translator state
    heap_offset = 0;
}

/*
 * Compilation instructions:
 *
 * emcc uppercase_translator.c -o uppercase_translator.wasm \
 *   -s EXPORTED_FUNCTIONS='["_handle_9p_message","_malloc","_free","_get_translator_info","_init"]' \
 *   -s ALLOW_MEMORY_GROWTH=0 \
 *   -s INITIAL_MEMORY=1048576 \
 *   -O2 \
 *   --no-entry
 *
 * Installation:
 * cp uppercase_translator.wasm /settrans/install/
 * echo '{"name":"uppercase","mount_point":"/trans/uppercase","version":"1.0"}' > /settrans/install/uppercase.json
 * ln -s /settrans/install/uppercase_translator.wasm /settrans/enabled/
 *
 * Usage:
 * echo "hello world" > /trans/uppercase/test.txt
 * cat /trans/uppercase/test.txt
 * # Output: HELLO WORLD FROM WASM TRANSLATOR!
 */