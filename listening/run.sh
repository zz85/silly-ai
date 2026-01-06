#!/bin/bash
# Wrapper to set Swift library path for screencapturekit
export DYLD_LIBRARY_PATH="/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx:$DYLD_LIBRARY_PATH"
exec "$(dirname "$0")/target/debug/listening" "$@" 2>&1 | grep -v "SwiftNativeNSObject"
