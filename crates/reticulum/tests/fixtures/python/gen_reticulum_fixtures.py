#!/usr/bin/env python3
import os
import sys
from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def ensure_reticulum_on_path() -> Path:
    root = repo_root()
    default_path = root.parent / "Reticulum"
    reticulum_path = Path(os.environ.get("RETICULUM_PY_PATH", str(default_path)))
    if not reticulum_path.exists():
        raise SystemExit(f"Reticulum python tree not found at {reticulum_path}")
    sys.path.insert(0, str(reticulum_path))
    return reticulum_path


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def main() -> None:
    ensure_reticulum_on_path()
    import RNS
    from RNS.Cryptography import Token
    from RNS.vendor import umsgpack

    root = repo_root()
    out_dir = root / "tests/fixtures/python/reticulum"
    config_dir = root / "tests/fixtures/python/.reticulum"
    config_dir.mkdir(parents=True, exist_ok=True)

    # Initialize Reticulum with isolated config dir
    RNS.Reticulum(configdir=str(config_dir), loglevel=RNS.LOG_CRITICAL)

    # Deterministic identity using fixed private key bytes
    prv_bytes = bytes(range(64))
    identity = RNS.Identity.from_bytes(prv_bytes)
    if identity is None:
        raise SystemExit("Failed to construct Identity from fixed bytes")

    write_bytes(out_dir / "identity.bin", prv_bytes)

    # Deterministic destination hash
    destination_hash = RNS.Destination.hash(identity, "lxmf", "delivery")
    write_bytes(out_dir / "destination_hash.bin", destination_hash)

    # Deterministic packet header (announce avoids encryption)
    destination = RNS.Destination(
        identity,
        RNS.Destination.OUT,
        RNS.Destination.SINGLE,
        "lxmf",
        "delivery",
    )
    packet = RNS.Packet(
        destination,
        b"fixture-payload",
        packet_type=RNS.Packet.ANNOUNCE,
        context=RNS.Packet.NONE,
        transport_type=RNS.Transport.BROADCAST,
        create_receipt=False,
    )
    packet.pack()
    write_bytes(out_dir / "packet_header.bin", packet.header)

    # Deterministic token encryption with fixed IV
    key = bytes(range(64))
    plaintext = b"reticulum-fixture-plaintext"
    token = Token(key)
    fixed_iv = bytes([0x42] * 16)
    original_urandom = os.urandom
    try:
        os.urandom = lambda n: fixed_iv if n == 16 else bytes([0x42] * n)
        ciphertext = token.encrypt(plaintext)
    finally:
        os.urandom = original_urandom

    write_bytes(out_dir / "crypto_key.bin", key)
    write_bytes(out_dir / "plaintext.bin", plaintext)
    write_bytes(out_dir / "encrypted_payload.bin", ciphertext)

    # Empty routing table placeholder
    write_bytes(out_dir / "routing_table.bin", umsgpack.packb([]))


if __name__ == "__main__":
    main()
