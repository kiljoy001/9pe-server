#!/bin/bash
#
# Compile WASM translators for 9PE server
#

set -e

EMCC_PATH="emcc"

# Check if emscripten is available
if ! command -v $EMCC_PATH &> /dev/null; then
    echo "❌ emcc not found. Please install Emscripten:"
    echo "   git clone https://github.com/emscripten-core/emsdk.git"
    echo "   cd emsdk"
    echo "   ./emsdk install latest"
    echo "   ./emsdk activate latest"
    echo "   source ./emsdk_env.sh"
    exit 1
fi

echo "🔧 Compiling WASM translators..."

# Create output directory
mkdir -p compiled_translators

# Compile uppercase translator
echo "📦 Compiling uppercase_translator.c..."
$EMCC_PATH uppercase_translator.c -o compiled_translators/uppercase_translator.wasm \
    -s EXPORTED_FUNCTIONS='["_handle_9p_message","_malloc","_free","_get_translator_info","_init"]' \
    -s ALLOW_MEMORY_GROWTH=0 \
    -s INITIAL_MEMORY=1048576 \
    -s STANDALONE_WASM=1 \
    -O2 \
    --no-entry

# Create metadata file
echo "📝 Creating metadata..."
cat > compiled_translators/uppercase_translator.json << EOF
{
    "name": "uppercase_translator",
    "mount_point": "/trans/uppercase",
    "version": "1.0.0",
    "description": "Converts all text to uppercase using WASM",
    "author": "9PE Examples",
    "exports": [
        "handle_9p_message",
        "malloc",
        "free",
        "get_translator_info",
        "init"
    ]
}
EOF

# Create installation script
cat > compiled_translators/install.sh << 'EOF'
#!/bin/bash
#
# Install compiled WASM translators
#

set -e

SETTRANS_DIR="${1:-./settrans}"

echo "🚀 Installing WASM translators to $SETTRANS_DIR"

# Create directories
mkdir -p "$SETTRANS_DIR/install"
mkdir -p "$SETTRANS_DIR/enabled"

# Install translator files
echo "📦 Installing uppercase_translator..."
cp uppercase_translator.wasm "$SETTRANS_DIR/install/"
cp uppercase_translator.json "$SETTRANS_DIR/install/"

# Enable translator (symlink)
ln -sf "../install/uppercase_translator.wasm" "$SETTRANS_DIR/enabled/"
ln -sf "../install/uppercase_translator.json" "$SETTRANS_DIR/enabled/"

echo "✅ Installation complete!"
echo ""
echo "Usage:"
echo "  1. Start 9PE server: ./9pe-server serve --path /tmp"
echo "  2. Access translator: echo 'hello' > /trans/uppercase/test.txt"
echo "  3. Read result: cat /trans/uppercase/test.txt"
echo "     Expected output: HELLO WORLD FROM WASM TRANSLATOR!"
EOF

chmod +x compiled_translators/install.sh

# Verify compilation
if [ -f "compiled_translators/uppercase_translator.wasm" ]; then
    WASM_SIZE=$(stat -c%s compiled_translators/uppercase_translator.wasm)
    echo "✅ uppercase_translator.wasm compiled successfully ($WASM_SIZE bytes)"
else
    echo "❌ Compilation failed"
    exit 1
fi

echo ""
echo "🎉 WASM translator compilation complete!"
echo ""
echo "Generated files:"
echo "  📁 compiled_translators/"
echo "    📦 uppercase_translator.wasm"
echo "    📝 uppercase_translator.json"
echo "    🔧 install.sh"
echo ""
echo "Next steps:"
echo "  cd compiled_translators"
echo "  ./install.sh /path/to/9pe-server/settrans"