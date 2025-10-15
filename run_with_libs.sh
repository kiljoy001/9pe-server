#!/bin/bash
export LD_LIBRARY_PATH=target/debug/deps:$LD_LIBRARY_PATH
exec "$@"
