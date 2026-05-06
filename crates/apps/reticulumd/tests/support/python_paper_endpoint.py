#!/usr/bin/env python3
import argparse
import atexit
import json
from pathlib import Path

import LXMF
import RNS
from LXMF import LXStamper


def start_reticulum(config_dir):
    Path(config_dir).mkdir(parents=True, exist_ok=True)
    reticulum = RNS.Reticulum(configdir=str(config_dir), loglevel=0)
    try:
        atexit.unregister(RNS.Reticulum.exit_handler)
    except Exception:
        pass
    return reticulum


def make_router(storage_dir):
    Path(storage_dir).mkdir(parents=True, exist_ok=True)
    return LXMF.LXMRouter(storagepath=str(storage_dir), enforce_stamps=False)


def paper_message_uri(config_dir, storage_dir, title, content):
    start_reticulum(config_dir)
    router = make_router(storage_dir)
    recipient = RNS.Identity()
    sender = RNS.Identity()
    recipient_destination = RNS.Destination(
        recipient,
        RNS.Destination.OUT,
        RNS.Destination.SINGLE,
        LXMF.APP_NAME,
        "delivery",
    )
    source = router.register_delivery_identity(sender, display_name="Python Paper Sender")
    message = LXMF.LXMessage(
        recipient_destination,
        source,
        content,
        title,
        desired_method=LXMF.LXMessage.PAPER,
    )
    message.pack()
    return {
        "recipient_private_key": recipient.get_private_key().hex(),
        "recipient_hash": recipient_destination.hash.hex(),
        "source_hash": source.hash.hex(),
        "uri": message.as_uri(finalise=False),
        "title": title,
        "content": content,
    }


def ingest_paper_uri(config_dir, storage_dir, recipient_private_key, uri):
    start_reticulum(config_dir)
    router = make_router(storage_dir)
    recipient = RNS.Identity.from_bytes(bytes.fromhex(recipient_private_key))
    if recipient is None:
        raise RuntimeError("invalid recipient identity")

    destination = router.register_delivery_identity(recipient, display_name="Python Paper Receiver")
    received = []

    def on_delivery(message):
        received.append(
            {
                "destination_hash": message.destination_hash.hex(),
                "source_hash": message.source_hash.hex(),
                "title": message.title_as_string(),
                "content": message.content_as_string(),
                "method": message.method,
                "signature_validated": message.signature_validated,
            }
        )

    router.register_delivery_callback(on_delivery)
    result = router.ingest_lxm_uri(uri, signal_local_delivery="local", allow_duplicate=True)
    return {
        "result": result,
        "recipient_hash": destination.hash.hex(),
        "received": received,
    }


def validate_wire_stamp(config_dir, storage_dir, wire_hex, target_cost, ticket_hex):
    start_reticulum(config_dir)
    make_router(storage_dir)
    message = LXMF.LXMessage.unpack_from_bytes(bytes.fromhex(wire_hex))
    tickets = None
    if ticket_hex is not None:
        tickets = [bytes.fromhex(ticket_hex)]
    valid = message.validate_stamp(target_cost, tickets=tickets)
    return {
        "valid": bool(valid),
        "stamp_value": message.stamp_value,
        "has_stamp": message.stamp is not None,
        "message_id": message.message_id.hex(),
    }


def make_stamped_wire(config_dir, storage_dir, title, content, target_cost, ticket_hex):
    start_reticulum(config_dir)
    router = make_router(storage_dir)
    recipient = RNS.Identity()
    sender = RNS.Identity()
    destination = RNS.Destination(
        recipient,
        RNS.Destination.OUT,
        RNS.Destination.SINGLE,
        LXMF.APP_NAME,
        "delivery",
    )
    source = router.register_delivery_identity(sender, display_name="Python Stamp Sender")
    message = LXMF.LXMessage(
        destination,
        source,
        content,
        title,
        desired_method=LXMF.LXMessage.DIRECT,
        stamp_cost=target_cost,
    )
    if ticket_hex is not None:
        message.outbound_ticket = bytes.fromhex(ticket_hex)
    message.defer_stamp = False
    message.pack()
    return {
        "wire_hex": message.packed.hex(),
        "destination_hash": destination.hash.hex(),
        "source_hash": source.hash.hex(),
        "message_id": message.message_id.hex(),
        "has_stamp": message.stamp is not None,
        "stamp_value": message.stamp_value,
        "ticket_hex": ticket_hex,
    }


def validate_pn_stamp(config_dir, storage_dir, transient_hex, target_cost):
    start_reticulum(config_dir)
    make_router(storage_dir)
    transient_id, lxm_data, value, stamp_data = LXStamper.validate_pn_stamp(
        bytes.fromhex(transient_hex),
        target_cost,
    )
    return {
        "valid": transient_id is not None,
        "transient_id": transient_id.hex() if transient_id is not None else None,
        "lxm_data_hex": lxm_data.hex() if lxm_data is not None else None,
        "stamp_value": value,
        "stamp_hex": stamp_data.hex() if stamp_data is not None else None,
    }


def make_pn_stamped_wire(config_dir, storage_dir, title, content, target_cost):
    start_reticulum(config_dir)
    router = make_router(storage_dir)
    recipient = RNS.Identity()
    sender = RNS.Identity()
    destination = RNS.Destination(
        recipient,
        RNS.Destination.OUT,
        RNS.Destination.SINGLE,
        LXMF.APP_NAME,
        "delivery",
    )
    source = router.register_delivery_identity(sender, display_name="Python PN Stamp Sender")
    message = LXMF.LXMessage(
        destination,
        source,
        content,
        title,
        desired_method=LXMF.LXMessage.DIRECT,
    )
    message.pack()
    transient_id = RNS.Identity.full_hash(message.packed)
    stamp, value = LXStamper.generate_stamp(
        transient_id,
        target_cost,
        expand_rounds=LXStamper.WORKBLOCK_EXPAND_ROUNDS_PN,
    )
    transient = message.packed + stamp
    return {
        "wire_hex": message.packed.hex(),
        "transient_hex": transient.hex(),
        "transient_id": transient_id.hex(),
        "stamp_hex": stamp.hex(),
        "stamp_value": value,
    }


def validate_peering_key(config_dir, storage_dir, peering_id_hex, key_hex, target_cost):
    start_reticulum(config_dir)
    make_router(storage_dir)
    peering_id = bytes.fromhex(peering_id_hex)
    key = bytes.fromhex(key_hex)
    return {
        "valid": bool(LXStamper.validate_peering_key(peering_id, key, target_cost)),
    }


def make_peering_key(config_dir, storage_dir, peering_id_hex, target_cost):
    start_reticulum(config_dir)
    make_router(storage_dir)
    peering_id = bytes.fromhex(peering_id_hex)
    key, value = LXStamper.generate_stamp(
        peering_id,
        target_cost,
        expand_rounds=LXStamper.WORKBLOCK_EXPAND_ROUNDS_PEERING,
    )
    return {
        "key_hex": key.hex(),
        "value": value,
    }


def _jsonable(value):
    if isinstance(value, bytes):
        try:
            return json.loads(value.decode("utf-8"))
        except Exception:
            return list(value)
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_jsonable(v) for v in value]
    if isinstance(value, tuple):
        return [_jsonable(v) for v in value]
    return value


def inspect_wire_fields(config_dir, storage_dir, wire_hex):
    start_reticulum(config_dir)
    make_router(storage_dir)
    message = LXMF.LXMessage.unpack_from_bytes(bytes.fromhex(wire_hex))
    return {
        "title": message.title_as_string(),
        "content": message.content_as_string(),
        "fields": _jsonable(message.fields),
    }


def make_field_wire(config_dir, storage_dir):
    start_reticulum(config_dir)
    router = make_router(storage_dir)
    recipient = RNS.Identity()
    sender = RNS.Identity()
    destination = RNS.Destination(
        recipient,
        RNS.Destination.OUT,
        RNS.Destination.SINGLE,
        LXMF.APP_NAME,
        "delivery",
    )
    source = router.register_delivery_identity(sender, display_name="Python Field Sender")
    message = LXMF.LXMessage(
        destination,
        source,
        "python field body",
        "python field title",
        fields={
            5: [["python.bin", b"\x01\x02\x03"]],
            112: b'{"sender":"python","type":"field-test"}',
        },
        desired_method=LXMF.LXMessage.DIRECT,
    )
    message.pack()
    return {
        "wire_hex": message.packed.hex(),
        "destination_hash": destination.hash.hex(),
        "source_hash": source.hash.hex(),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config-dir", required=True)
    parser.add_argument("--storage-dir", required=True)
    sub = parser.add_subparsers(dest="command", required=True)

    make = sub.add_parser("make-paper-uri")
    make.add_argument("--title", required=True)
    make.add_argument("--content", required=True)

    ingest = sub.add_parser("ingest-paper-uri")
    ingest.add_argument("--recipient-private-key", required=True)
    ingest.add_argument("--uri", required=True)

    validate = sub.add_parser("validate-wire-stamp")
    validate.add_argument("--wire-hex", required=True)
    validate.add_argument("--target-cost", required=True, type=int)
    validate.add_argument("--ticket-hex")

    stamped = sub.add_parser("make-stamped-wire")
    stamped.add_argument("--title", required=True)
    stamped.add_argument("--content", required=True)
    stamped.add_argument("--target-cost", required=True, type=int)
    stamped.add_argument("--ticket-hex")

    pn_validate = sub.add_parser("validate-pn-stamp")
    pn_validate.add_argument("--transient-hex", required=True)
    pn_validate.add_argument("--target-cost", required=True, type=int)

    pn_stamped = sub.add_parser("make-pn-stamped-wire")
    pn_stamped.add_argument("--title", required=True)
    pn_stamped.add_argument("--content", required=True)
    pn_stamped.add_argument("--target-cost", required=True, type=int)

    peering_validate = sub.add_parser("validate-peering-key")
    peering_validate.add_argument("--peering-id-hex", required=True)
    peering_validate.add_argument("--key-hex", required=True)
    peering_validate.add_argument("--target-cost", required=True, type=int)

    peering_make = sub.add_parser("make-peering-key")
    peering_make.add_argument("--peering-id-hex", required=True)
    peering_make.add_argument("--target-cost", required=True, type=int)

    inspect_fields = sub.add_parser("inspect-wire-fields")
    inspect_fields.add_argument("--wire-hex", required=True)

    sub.add_parser("make-field-wire")

    args = parser.parse_args()
    if args.command == "make-paper-uri":
        result = paper_message_uri(args.config_dir, args.storage_dir, args.title, args.content)
    elif args.command == "ingest-paper-uri":
        result = ingest_paper_uri(
            args.config_dir,
            args.storage_dir,
            args.recipient_private_key,
            args.uri,
        )
    elif args.command == "validate-wire-stamp":
        result = validate_wire_stamp(
            args.config_dir,
            args.storage_dir,
            args.wire_hex,
            args.target_cost,
            args.ticket_hex,
        )
    elif args.command == "make-stamped-wire":
        result = make_stamped_wire(
            args.config_dir,
            args.storage_dir,
            args.title,
            args.content,
            args.target_cost,
            args.ticket_hex,
        )
    elif args.command == "validate-pn-stamp":
        result = validate_pn_stamp(
            args.config_dir,
            args.storage_dir,
            args.transient_hex,
            args.target_cost,
        )
    elif args.command == "make-pn-stamped-wire":
        result = make_pn_stamped_wire(
            args.config_dir,
            args.storage_dir,
            args.title,
            args.content,
            args.target_cost,
        )
    elif args.command == "validate-peering-key":
        result = validate_peering_key(
            args.config_dir,
            args.storage_dir,
            args.peering_id_hex,
            args.key_hex,
            args.target_cost,
        )
    elif args.command == "make-peering-key":
        result = make_peering_key(
            args.config_dir,
            args.storage_dir,
            args.peering_id_hex,
            args.target_cost,
        )
    elif args.command == "inspect-wire-fields":
        result = inspect_wire_fields(
            args.config_dir,
            args.storage_dir,
            args.wire_hex,
        )
    elif args.command == "make-field-wire":
        result = make_field_wire(args.config_dir, args.storage_dir)
    else:
        raise RuntimeError(f"unsupported command: {args.command}")

    print(json.dumps(result, sort_keys=True), flush=True)


if __name__ == "__main__":
    main()
