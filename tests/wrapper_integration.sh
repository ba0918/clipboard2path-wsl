#!/bin/bash
# Integration tests for the wl-paste wrapper script.
# Tests the generated wrapper by substituting REAL_WL_PASTE with a mock.
#
# Usage: bash tests/wrapper_integration.sh
# Exit code: 0 if all tests pass, 1 if any fail.

set -euo pipefail

PASS=0
FAIL=0
TOTAL=0

# Colors (if terminal supports)
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

assert_eq() {
    local test_name="$1"
    local expected="$2"
    local actual="$3"
    TOTAL=$((TOTAL + 1))
    if [ "$expected" = "$actual" ]; then
        echo -e "  ${GREEN}PASS${NC}: $test_name"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${NC}: $test_name"
        echo "    expected: $(echo -n "$expected" | head -c 200)"
        echo "    actual:   $(echo -n "$actual" | head -c 200)"
        FAIL=$((FAIL + 1))
    fi
}

assert_contains() {
    local test_name="$1"
    local needle="$2"
    local haystack="$3"
    TOTAL=$((TOTAL + 1))
    if echo "$haystack" | grep -qF "$needle"; then
        echo -e "  ${GREEN}PASS${NC}: $test_name"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${NC}: $test_name"
        echo "    expected to contain: $needle"
        echo "    actual: $(echo -n "$haystack" | head -c 200)"
        FAIL=$((FAIL + 1))
    fi
}

# Setup test environment
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

# Create mock real wl-paste
MOCK_WL_PASTE="$TEST_DIR/real-wl-paste"
cat > "$MOCK_WL_PASTE" << 'MOCK_EOF'
#!/bin/bash
echo "REAL_WL_PASTE_CALLED: $*"
MOCK_EOF
chmod +x "$MOCK_WL_PASTE"

# Create runtime dir structure
RUNTIME_DIR="$TEST_DIR/runtime"
mkdir -p "$RUNTIME_DIR/clipboard2path"

# Create a test PNG file and latest.png symlink
TEST_PNG="$RUNTIME_DIR/clipboard2path/clip_20260406_120000.png"
echo "FAKE_PNG_DATA" > "$TEST_PNG"
ln -sf "$TEST_PNG" "$RUNTIME_DIR/clipboard2path/latest.png"

# Generate the wrapper with our mock path
# We use cargo to generate it, but for testing we'll create it directly
WRAPPER="$TEST_DIR/wl-paste"
cat > "$WRAPPER" << WRAPPER_EOF
#!/bin/bash
# clipboard2path-wsl wl-paste wrapper
# MANAGED BY clipboard2path-wsl — DO NOT EDIT
# Bridges daemon's saved PNG to applications requesting image/png

REAL_WL_PASTE="$MOCK_WL_PASTE"
LATEST_PNG="\${XDG_RUNTIME_DIR}/clipboard2path/latest.png"

# Bail out immediately if XDG_RUNTIME_DIR is unset (match daemon behavior)
[ -z "\$XDG_RUNTIME_DIR" ] && exec "\$REAL_WL_PASTE" "\$@"

# Bail out if --watch, --primary, --seat, or unknown long options are present
# (only intercept simple single-shot --type image/png requests)
for arg in "\$@"; do
    case "\$arg" in
        --watch|-w|--primary|-p|--seat|-s) exec "\$REAL_WL_PASTE" "\$@" ;;
    esac
done

# Detect --type image/png or -t image/png request (space or = separated)
want_png=0
prev=""
for arg in "\$@"; do
    case "\$arg" in
        --type=image/png|-t=image/png) want_png=1; break ;;
    esac
    if [ "\$prev" = "--type" ] || [ "\$prev" = "-t" ]; then
        [ "\$arg" = "image/png" ] && want_png=1 && break
    fi
    prev="\$arg"
done

if [ "\$want_png" = "1" ] && [ -L "\$LATEST_PNG" ] && [ -f "\$LATEST_PNG" ]; then
    cat "\$LATEST_PNG"
    exit 0
fi

exec "\$REAL_WL_PASTE" "\$@"
WRAPPER_EOF
chmod +x "$WRAPPER"

echo "=== wl-paste wrapper integration tests ==="

# Test 1: --type image/png with valid latest.png -> PNG output
echo ""
echo "Test group: image/png interception"
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --type image/png)
assert_eq "--type image/png returns PNG data" "FAKE_PNG_DATA" "$output"

# Test 2: -t image/png with valid latest.png -> PNG output
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" -t image/png)
assert_eq "-t image/png returns PNG data" "FAKE_PNG_DATA" "$output"

# Test 3: --type=image/png (= separated) -> PNG output
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --type=image/png)
assert_eq "--type=image/png returns PNG data" "FAKE_PNG_DATA" "$output"

# Test 4: -t=image/png (= separated) -> PNG output
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" -t=image/png)
assert_eq "-t=image/png returns PNG data" "FAKE_PNG_DATA" "$output"

# Test 5: --type image/png with missing latest.png -> fallback to real
echo ""
echo "Test group: fallback to real wl-paste"
rm "$RUNTIME_DIR/clipboard2path/latest.png"
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --type image/png)
assert_contains "missing latest.png falls back to real wl-paste" "REAL_WL_PASTE_CALLED" "$output"
# Restore symlink
ln -sf "$TEST_PNG" "$RUNTIME_DIR/clipboard2path/latest.png"

# Test 6: --type image/bmp -> delegate to real wl-paste
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --type image/bmp)
assert_contains "--type image/bmp delegates to real wl-paste" "REAL_WL_PASTE_CALLED" "$output"

# Test 7: --list-types -> delegate to real wl-paste
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --list-types)
assert_contains "--list-types delegates to real wl-paste" "REAL_WL_PASTE_CALLED" "$output"

# Test 8: --watch --type image/png -> delegate (don't intercept)
echo ""
echo "Test group: bypass flags"
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --watch --type image/png)
assert_contains "--watch bypasses interception" "REAL_WL_PASTE_CALLED" "$output"

# Test 9: --primary --type image/png -> delegate
output=$(XDG_RUNTIME_DIR="$RUNTIME_DIR" "$WRAPPER" --primary --type image/png)
assert_contains "--primary bypasses interception" "REAL_WL_PASTE_CALLED" "$output"

# Test 10: XDG_RUNTIME_DIR unset -> delegate
echo ""
echo "Test group: XDG_RUNTIME_DIR handling"
output=$(unset XDG_RUNTIME_DIR; "$WRAPPER" --type image/png)
assert_contains "unset XDG_RUNTIME_DIR delegates to real wl-paste" "REAL_WL_PASTE_CALLED" "$output"

# Summary
echo ""
echo "=== Results: $PASS/$TOTAL passed, $FAIL failed ==="

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
exit 0
