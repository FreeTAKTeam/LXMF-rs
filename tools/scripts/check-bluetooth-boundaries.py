#!/usr/bin/env python3
"""Exact feature and package policy for the protocol-neutral Bluetooth leaf."""
from pathlib import Path
import tomllib

root = Path(__file__).resolve().parents[2]
leaf = tomllib.loads((root / "crates/libs/rns-bluetooth/Cargo.toml").read_text())
assert leaf["package"]["name"] == "reticulum-rs-bluetooth"
assert leaf["lib"]["name"] == "rns_bluetooth"
assert leaf["package"]["edition"] == "2024"
assert leaf["package"]["rust-version"] == "1.85"
assert leaf["features"]["default"] == []
assert not leaf.get("dependencies"), "core must remain platform- and protocol-neutral"
linux = leaf["target"]['cfg(target_os = "linux")']["dependencies"]
assert set(linux) == {"bluer", "tokio", "futures-util"}
assert all(dep["optional"] for dep in linux.values())
assert linux["bluer"]["version"] == "=0.17.4"
assert set(leaf["features"]["bluez"]) == {f"dep:{name}" for name in linux}
sdk = tomllib.loads((root / "crates/libs/lxmf-sdk/Cargo.toml").read_text())
assert sdk["dependencies"]["rns-bluetooth"]["optional"]
assert sdk["features"]["mobile-ble-adapter"] == ["std", "dep:rns-bluetooth"]
assert "mobile-ble-adapter" not in sdk["features"]["default"]
workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
assert not workspace["dependencies"]["rns-bluetooth"]["default-features"]
workflow = (root / ".github/workflows/crates-io-publish.yml").read_text()
first = workflow.index('crates/libs/rns-bluetooth/Cargo.toml')
assert first < workflow.index('crates/libs/lxmf-sdk/Cargo.toml')
assert first < workflow.index('crates/libs/rns-transport/Cargo.toml')
print("Bluetooth feature isolation and publish order: ok")
