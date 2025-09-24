/*
 * Modern Plumber WASM Translator
 *
 * Inter-application messaging system that routes messages based on content patterns.
 *
 * Usage via 9P.e files:
 *   echo "file.txt:123" > /translators/plumber/send  -> routes to editor
 *   cat /translators/plumber/ports/edit/messages     -> see edit requests
 *   cat /translators/plumber/log                     -> see all routing
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <regex.h>

// Export functions for WASM
#define WASM_EXPORT __attribute__((visibility("default")))

// Message routing state
typedef struct {
    char pattern[256];
    char target_port[64];
    int priority;
} PlumbRule;

typedef struct {
    char data[1024];
    char src[64];
    char dst[64];
    char data_type[32];
} PlumbMessage;

// Global state
static PlumbRule rules[32];
static int num_rules = 0;
static PlumbMessage message_log[100];
static int log_size = 0;

// Port message queues
static PlumbMessage edit_messages[10];
static int edit_count = 0;
static PlumbMessage web_messages[10];
static int web_count = 0;
static PlumbMessage terminal_messages[10];
static int terminal_count = 0;

WASM_EXPORT
void init() {
    // Initialize default plumbing rules

    // Rule 1: file:line -> edit port (priority 100)
    strcpy(rules[0].pattern, "^[^:]+:[0-9]+$");
    strcpy(rules[0].target_port, "edit");
    rules[0].priority = 100;

    // Rule 2: URLs -> web port (priority 90)
    strcpy(rules[1].pattern, "^https?://");
    strcpy(rules[1].target_port, "web");
    rules[1].priority = 90;

    // Rule 3: user@host -> terminal port (priority 80)
    strcpy(rules[2].pattern, "^[a-zA-Z0-9_-]+@[a-zA-Z0-9.-]+$");
    strcpy(rules[2].target_port, "terminal");
    rules[2].priority = 80;

    // Rule 4: catch-all -> edit port (priority 1)
    strcpy(rules[3].pattern, ".*");
    strcpy(rules[3].target_port, "edit");
    rules[3].priority = 1;

    num_rules = 4;
    log_size = 0;
    edit_count = 0;
    web_count = 0;
    terminal_count = 0;
}

// Simple regex matching function (simplified for WASM)
static int matches_pattern(const char* pattern, const char* text) {
    // Simplified pattern matching for demo
    if (strstr(pattern, "^[^:]+:[0-9]+$") != NULL) {
        // Check for file:line pattern
        char* colon = strchr(text, ':');
        if (colon != NULL) {
            // Check if part after colon is a number
            char* end;
            strtol(colon + 1, &end, 10);
            return (*end == '\0');  // Valid if consumed entire string
        }
        return 0;
    } else if (strstr(pattern, "^https?://") != NULL) {
        return (strncmp(text, "http://", 7) == 0 || strncmp(text, "https://", 8) == 0);
    } else if (strstr(pattern, "@") != NULL) {
        return (strchr(text, '@') != NULL);
    } else if (strcmp(pattern, ".*") == 0) {
        return 1;  // Catch-all
    }
    return 0;
}

// Route message to appropriate port
static void route_message(PlumbMessage* msg) {
    // Find matching rule (rules are in priority order)
    for (int i = 0; i < num_rules; i++) {
        if (matches_pattern(rules[i].pattern, msg->data)) {
            strcpy(msg->dst, rules[i].target_port);

            // Add to appropriate port queue
            if (strcmp(msg->dst, "edit") == 0 && edit_count < 10) {
                edit_messages[edit_count++] = *msg;
            } else if (strcmp(msg->dst, "web") == 0 && web_count < 10) {
                web_messages[web_count++] = *msg;
            } else if (strcmp(msg->dst, "terminal") == 0 && terminal_count < 10) {
                terminal_messages[terminal_count++] = *msg;
            }
            break;
        }
    }

    // Add to global log
    if (log_size < 100) {
        message_log[log_size++] = *msg;
    }
}

WASM_EXPORT
int handle_9p_message(const char* path, const char* operation, const char* data, char* response, int max_response) {
    if (strcmp(operation, "write") == 0) {
        if (strcmp(path, "/plumb/send") == 0) {
            // Handle message sending
            PlumbMessage msg = {0};
            strncpy(msg.data, data, sizeof(msg.data) - 1);
            strcpy(msg.src, "user");
            strcpy(msg.data_type, "text");

            route_message(&msg);

            snprintf(response, max_response, "routed to %s", msg.dst);
            return strlen(response);
        }
    } else if (strcmp(operation, "read") == 0) {
        if (strcmp(path, "/plumb/log") == 0) {
            // Return message log
            int pos = 0;
            pos += snprintf(response + pos, max_response - pos, "# Plumber Message Log\n");

            for (int i = 0; i < log_size && pos < max_response - 100; i++) {
                pos += snprintf(response + pos, max_response - pos,
                    "[%d] %s -> %s (%s): %s\n",
                    i + 1, message_log[i].src, message_log[i].dst,
                    message_log[i].data_type, message_log[i].data);
            }
            return pos;
        } else if (strcmp(path, "/plumb/ports/edit/messages") == 0) {
            // Return edit port messages
            int pos = 0;
            pos += snprintf(response + pos, max_response - pos, "# Edit Port Messages\n");

            for (int i = 0; i < edit_count && pos < max_response - 100; i++) {
                pos += snprintf(response + pos, max_response - pos,
                    "%d. %s\n", i + 1, edit_messages[i].data);
            }

            if (edit_count == 0) {
                pos += snprintf(response + pos, max_response - pos, "No messages\n");
            }
            return pos;
        } else if (strcmp(path, "/plumb/ports/web/messages") == 0) {
            // Return web port messages
            int pos = 0;
            pos += snprintf(response + pos, max_response - pos, "# Web Port Messages\n");

            for (int i = 0; i < web_count && pos < max_response - 100; i++) {
                pos += snprintf(response + pos, max_response - pos,
                    "%d. %s\n", i + 1, web_messages[i].data);
            }

            if (web_count == 0) {
                pos += snprintf(response + pos, max_response - pos, "No messages\n");
            }
            return pos;
        } else if (strcmp(path, "/plumb/ports/terminal/messages") == 0) {
            // Return terminal port messages
            int pos = 0;
            pos += snprintf(response + pos, max_response - pos, "# Terminal Port Messages\n");

            for (int i = 0; i < terminal_count && pos < max_response - 100; i++) {
                pos += snprintf(response + pos, max_response - pos,
                    "%d. %s\n", i + 1, terminal_messages[i].data);
            }

            if (terminal_count == 0) {
                pos += snprintf(response + pos, max_response - pos, "No messages\n");
            }
            return pos;
        } else if (strcmp(path, "/plumb/rules") == 0) {
            // Return plumbing rules
            int pos = 0;
            pos += snprintf(response + pos, max_response - pos, "# Plumber Rules (priority order)\n");

            for (int i = 0; i < num_rules && pos < max_response - 100; i++) {
                pos += snprintf(response + pos, max_response - pos,
                    "%d. [%d] %s -> %s\n",
                    i + 1, rules[i].priority, rules[i].pattern, rules[i].target_port);
            }
            return pos;
        } else if (strcmp(path, "/plumb/ports") == 0) {
            // List available ports
            snprintf(response, max_response,
                "edit (%d messages)\nweb (%d messages)\nterminal (%d messages)\n",
                edit_count, web_count, terminal_count);
            return strlen(response);
        }
    }

    // Default help response
    snprintf(response, max_response,
        "Modern Plumber - Inter-application messaging\n\n"
        "Usage:\n"
        "  echo \"file.txt:123\" > /plumb/send      # Edit file at line 123\n"
        "  echo \"https://example.com\" > /plumb/send  # Open URL\n"
        "  echo \"user@host\" > /plumb/send        # SSH connection\n\n"
        "Query:\n"
        "  cat /plumb/log                          # Message routing log\n"
        "  cat /plumb/ports/edit/messages          # Edit requests\n"
        "  cat /plumb/ports/web/messages           # Web requests\n"
        "  cat /plumb/rules                        # Routing rules\n");

    return strlen(response);
}

WASM_EXPORT
const char* get_translator_info() {
    return "plumber:1.0:Inter-application messaging via pattern matching";
}

// Standard WASM exports
WASM_EXPORT
void* malloc(size_t size) {
    return malloc(size);
}

WASM_EXPORT
void free(void* ptr) {
    free(ptr);
}