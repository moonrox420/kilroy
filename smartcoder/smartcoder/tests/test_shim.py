"""Quick test: verify the compat shim works when run directly."""

import sys
from pathlib import Path

script_dir = str(Path(__file__).resolve().parent)
parent_dir = str(Path(__file__).resolve().parent.parent.parent)

# Mimic what the shim does
if script_dir in sys.path:
    sys.path.remove(script_dir)
if parent_dir not in sys.path:
    sys.path.insert(0, parent_dir)

import kilroy_smartcoder

print("SFile:", kilroy_smartcoder.__file__)

import smartcoder

print("smartcoder package:", smartcoder.__file__)
from smartcoder.runtime.config import AppConfig

print("AppConfig OK:", AppConfig().backend)
