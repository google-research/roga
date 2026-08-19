#!/bin/bash
# Copyright 2026 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Script to run binsec verification on constant-time functions.
# Uses gen_verify.py to auto-generate .ini scripts with allocator stubs.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="$HOME/bin:$PATH"
WORKSPACE_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Accept binary path as optional first argument, or default to cargo target path
if [ -n "$1" ]; then
    BINARY_PATH="$1"
else
    BINARY_PATH="${WORKSPACE_DIR}/target/release/verification_harness"
    if [ ! -f "${BINARY_PATH}" ]; then
        echo "Building verification harness with Cargo..."
        cargo build --release --bin verification_harness --manifest-path "${WORKSPACE_DIR}/crates/oram/Cargo.toml"
    fi
fi

if [ ! -f "${BINARY_PATH}" ]; then
    echo "Error: Binary not found at ${BINARY_PATH}"
    echo "Usage: $0 [path/to/verification_harness]"
    exit 1
fi

echo "Using binary: ${BINARY_PATH}"

# Check if binsec is installed
if ! command -v binsec &> /dev/null; then
    echo "Warning: 'binsec' command not found in PATH."
    echo "Please install binsec (https://github.com/binsec/binsec) and make sure it is in your PATH."
    exit 1
fi

# Auto-generate .ini scripts in memory and run verification via stdin
echo "Running binsec verification via stdin piping..."
python3 "${SCRIPT_DIR}/gen_verify.py" "${BINARY_PATH}" --run

