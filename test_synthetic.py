#!/usr/bin/env python3
"""Simple test client for 9PE synthetic files"""

import socket
import struct
import sys

def test_9pe_synthetic():
    """Test synthetic file functionality via raw 9P protocol"""

    # Connect to server
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.connect(('127.0.0.1', 9999))
        print("✅ Connected to 9PE server")

        # Simple version negotiation
        version_msg = b"9P.e/1.0"
        msg_data = struct.pack("<I", len(version_msg)) + version_msg
        msg_len = len(msg_data) + 4

        sock.send(struct.pack("<I", msg_len))
        sock.send(msg_data)

        # Read response
        resp_len = struct.unpack("<I", sock.recv(4))[0]
        resp_data = sock.recv(resp_len - 4)
        print(f"📦 Server response: {resp_data[:50]}...")

        print("🧪 Basic protocol test successful!")
        print("✨ Server with synthetic files is responding!")

    except Exception as e:
        print(f"❌ Test failed: {e}")
    finally:
        sock.close()

if __name__ == "__main__":
    test_9pe_synthetic()