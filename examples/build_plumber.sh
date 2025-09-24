#!/bin/bash

# Build Modern Plumber WASM Translator
# Compiles plumber_translator.c to WebAssembly for use with 9P.e server

set -e

echo "🔗 Building Modern Plumber WASM Translator..."

# Check for clang with WASM support
if ! command -v clang &> /dev/null; then
    echo "❌ Error: clang not found. Install clang with WASM support."
    echo "   Ubuntu: sudo apt install clang lld"
    echo "   macOS: brew install llvm"
    exit 1
fi

# Compile to WASM
echo "📦 Compiling plumber_translator.c to WASM..."

clang --target=wasm32 \
      --no-standard-libraries \
      -Wl,--export-dynamic \
      -Wl,--no-entry \
      -Wl,--allow-undefined \
      -Wl,--export=handle_9p_message \
      -Wl,--export=get_translator_info \
      -Wl,--export=init \
      -Wl,--export=malloc \
      -Wl,--export=free \
      -O2 \
      -o plumber_translator.wasm \
      plumber_translator.c

if [ $? -eq 0 ]; then
    echo "✅ Successfully built plumber_translator.wasm"
    echo "📊 File size: $(ls -lh plumber_translator.wasm | awk '{print $5}')"

    # Show exports
    echo "🔍 WASM exports:"
    if command -v wasm-objdump &> /dev/null; then
        wasm-objdump -x plumber_translator.wasm | grep -A 20 "Export section"
    else
        echo "   (install wabt tools to see exports: apt install wabt)"
    fi

    echo ""
    echo "🚀 Usage:"
    echo "   1. Copy plumber_translator.wasm to your 9P.e server"
    echo "   2. Load translator: echo 'plumber_translator.wasm' > /translators/load"
    echo "   3. Send messages: echo 'file.txt:123' > /plumb/send"
    echo "   4. Read results: cat /plumb/ports/edit/messages"
    echo ""
    echo "📖 Available endpoints:"
    echo "   /plumb/send                    - Send messages (write)"
    echo "   /plumb/log                     - Message routing log"
    echo "   /plumb/ports/edit/messages     - Edit requests"
    echo "   /plumb/ports/web/messages      - Web requests"
    echo "   /plumb/ports/terminal/messages - Terminal requests"
    echo "   /plumb/rules                   - Routing rules"

else
    echo "❌ Failed to build WASM translator"
    exit 1
fi