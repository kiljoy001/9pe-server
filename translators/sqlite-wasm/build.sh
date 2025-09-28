#!/bin/bash

# Build script for SQLite WASM Translator

echo "🔨 Building SQLite WASM Translator..."

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "❌ wasm-pack not found. Installing..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

# Build the WASM package
echo "📦 Compiling to WASM..."
wasm-pack build --target web --out-dir pkg

if [ $? -eq 0 ]; then
    echo "✅ WASM build successful!"
    echo "📁 Output files:"
    ls -la pkg/
else
    echo "❌ WASM build failed!"
    exit 1
fi

echo "🎉 SQLite WASM Translator ready!"