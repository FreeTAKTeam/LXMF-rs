#!/usr/bin/env python3
"""Validate MeshChat specification files without importing or starting MeshChat.

Requires PyYAML and jsonschema. No network access is performed. --source-root
adds AST coverage checks; --oas-schema adds an independently supplied official
OpenAPI 3.1 document meta-schema. The report distinguishes skipped checks.
"""
from __future__ import annotations
import argparse
import ast
import hashlib
import json
import re
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import yaml
from jsonschema import Draft202012Validator

PIN = 'df5aea94eab7f4be1cdfef494446e9d6979aed77'
BLOB = 'd574dda5222153b64d42fc7a945c297d2d436c3e'
HTTP_METHODS = {'get', 'post', 'put', 'patch', 'delete', 'head', 'options', 'trace'}


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding='utf-8'))


def walk(value: Any):
    yield value
    if isinstance(value, dict):
        for item in value.values():
            yield from walk(item)
    elif isinstance(value, list):
        for item in value:
            yield from walk(item)


def resolve_pointer(document: Any, pointer: str) -> Any:
    if not pointer.startswith('#/'):
        raise ValueError(f'Expected local JSON reference, received {pointer}')
    value = document
    for segment in pointer[2:].split('/'):
        key = segment.replace('~1', '/').replace('~0', '~')
        value = value[int(key)] if isinstance(value, list) else value[key]
    return value


def validator(document: dict, pointer: str) -> Draft202012Validator:
    root = {'$schema': 'https://json-schema.org/draft/2020-12/schema', '$ref': pointer}
    if 'components' in document:
        root['components'] = document['components']
    if '$defs' in document:
        root['$defs'] = document['$defs']
    return Draft202012Validator(root)


def literal_dict(node: ast.AST) -> dict[str, ast.AST]:
    if not isinstance(node, ast.Dict):
        return {}
    return {k.value: v for k, v in zip(node.keys, node.values)
            if isinstance(k, ast.Constant) and isinstance(k.value, str)}


def source_checks(source: Path, oas: dict, ws: dict, protocol: dict, inventory: dict) -> dict:
    raw = (source / 'meshchat.py').read_bytes()
    blob = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\x00' + raw).hexdigest()
    if blob != BLOB:
        raise AssertionError(f'Wrong meshchat.py blob: {blob}; expected {BLOB}')
    tree = ast.parse(raw.decode())
    found = {}
    for node in ast.walk(tree):
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        for dec in node.decorator_list:
            if (isinstance(dec, ast.Call) and isinstance(dec.func, ast.Attribute)
                    and isinstance(dec.func.value, ast.Name) and dec.func.value.id == 'routes'):
                found[(dec.func.attr, ast.literal_eval(dec.args[0]))] = (node, dec.lineno)
    declared = {(method, path) for path, item in oas['paths'].items()
                for method in item if method in HTTP_METHODS}
    if set(found) != declared:
        raise AssertionError(f'Route mismatch: missing={set(found)-declared}, extra={declared-set(found)}')
    for record in inventory['routes']:
        node, start = found[(record['method'].lower(), record['path'])]
        assert record['source']['lines'] == [start, node.end_lineno], record

    # Check every explicit JSON response status and top-level response key.
    explicit_json_returns = 0
    observed_responses = []
    for (method, path), (node, _) in found.items():
        operation = oas['paths'][path][method]
        for call in ast.walk(node):
            if not (isinstance(call, ast.Call) and isinstance(call.func, ast.Attribute)
                    and isinstance(call.func.value, ast.Name) and call.func.value.id == 'web'
                    and call.func.attr == 'json_response' and call.args):
                continue
            status = next((str(ast.literal_eval(k.value)) for k in call.keywords if k.arg == 'status'), '200')
            assert status in operation['responses'], (method, path, status)
            fields = literal_dict(call.args[0])
            response_schema = operation['responses'][status]['content']['application/json']['schema']
            if '$ref' in response_schema:
                response_schema = resolve_pointer(oas, response_schema['$ref'])
            assert set(fields) == set(response_schema.get('properties', {})), (method, path, fields)
            for key, value in fields.items():
                expected = response_schema['properties'][key]
                if 'const' in expected and isinstance(value, ast.Constant):
                    assert value.value == expected['const'], (method, path, value.value, expected)
            explicit_json_returns += 1
            literal_message = fields.get('message')
            observed_responses.append({'method': method.upper(), 'path': path, 'status': int(status),
                                       'line': call.lineno, 'keys': sorted(fields),
                                       'literalMessage': literal_message.value if isinstance(literal_message, ast.Constant) else None})

    emitted = set()
    emitted_variants = set()
    for node in ast.walk(tree):
        fields = literal_dict(node)
        kind = fields.get('type')
        if isinstance(kind, ast.Constant) and isinstance(kind.value, str):
            emitted.add(kind.value)
            nested = fields.get('nomadnet_file_download') or fields.get('nomadnet_page_download')
            status = literal_dict(nested).get('status')
            emitted_variants.add((kind.value, status.value if isinstance(status, ast.Constant) else None))
    server_messages = [m for m in protocol['messages'] if m['direction'] == 'server_to_client']
    assert emitted == {m['type'] for m in server_messages}, 'Server event type coverage mismatch'
    described_variants = set()
    for message in server_messages:
        schema = ws['$defs'][message['name']]
        props = schema['properties']
        nested = props.get('nomadnet_file_download') or props.get('nomadnet_page_download')
        status = nested['properties']['status']['const'] if nested else None
        described_variants.add((message['type'], status))
        # Each source range must actually contain the documented emitted type.
        start, end = message['source'][0]['lines']
        span = '\n'.join(raw.decode().splitlines()[start - 1:end])
        assert '"' + message['type'] + '"' in span, (message['name'], start, end)
    assert emitted_variants == described_variants, 'Server variant coverage mismatch'

    dispatch = next(n for n in ast.walk(tree) if isinstance(n, ast.AsyncFunctionDef)
                    and n.name == 'on_websocket_data_received')
    commands = {c.value for n in ast.walk(dispatch) if isinstance(n, ast.Compare)
                and isinstance(n.left, ast.Name) and n.left.id == '_type'
                for c in n.comparators if isinstance(c, ast.Constant)}
    assert commands == {m['type'] for m in protocol['messages'] if m['direction'] == 'client_to_server'}

    # Verify key sets returned by the shared serializers.
    helpers = {'get_config_dict': 'Config', 'convert_audio_call_to_dict': 'AudioCall',
               'convert_lxmf_message_to_dict': 'LiveLxmfMessage',
               'convert_db_lxmf_message_to_dict': 'StoredLxmfMessage',
               'convert_db_announce_to_dict': 'Announce', 'convert_db_favourite_to_dict': 'Favourite'}
    for helper, schema in helpers.items():
        fn = next(n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == helper)
        returns = [n for n in fn.body if isinstance(n, ast.Return) and isinstance(n.value, ast.Dict)]
        assert len(returns) == 1, helper
        keys = set(literal_dict(returns[0].value))
        assert keys == set(oas['components']['schemas'][schema]['properties']), (helper, keys)
        assert keys == set(oas['components']['schemas'][schema]['required']), (helper, 'required')

    # Scan frontend API call literals, normalizing template interpolation to path parameters.
    frontend_calls = []
    call_pattern = re.compile(r'(?:window\.)?axios\.(get|post|patch|delete|put)\(\s*([\'"`])(/api/v1[^\'"`]+)\2')
    api_patterns = [(m, re.compile('^' + re.sub(r'\\\{[^}]+\\\}', r'[^/]+', re.escape(p)) + '$'))
                    for m, p in declared]
    for file in sorted((source / 'src/frontend').rglob('*')):
        if file.suffix not in {'.vue', '.js'} or not file.is_file():
            continue
        text = file.read_text(encoding='utf-8')
        for match in call_pattern.finditer(text):
            method, _, original = match.groups()
            normalized = re.sub(r'\$\{[^}]+\}', 'placeholder', original)
            assert any(m == method and pattern.match(normalized) for m, pattern in api_patterns), (file, original)
            frontend_calls.append({'file': str(file.relative_to(source)),
                                   'line': text[:match.start()].count('\n') + 1,
                                   'method': method.upper(), 'pathExpression': original})
    audited_files = {Path('meshchat.py'), Path('database.py'), Path('requirements.txt'), Path('package.json'),
                     Path('vite.config.js'), Path('src/backend/interface_editor.py'),
                     Path('src/backend/interface_config_parser.py'), Path('src/backend/audio_call_manager.py'),
                     Path('src/frontend/js/WebSocketConnection.js'),
                     Path('src/frontend/public/assets/proto/audio_call.proto')}
    audited_files.update(Path(x['file']) for x in frontend_calls)
    source_manifest = [{'path': str(p), 'sha256': hashlib.sha256((source / p).read_bytes()).hexdigest(),
                        'lines': len((source / p).read_text().splitlines()),
                        'url': f'https://github.com/liamcottle/reticulum-meshchat/blob/{PIN}/{p}'}
                       for p in sorted(audited_files)]
    return {'status': 'passed', 'sourceBlob': blob, 'routeCount': len(found),
            'explicitJsonResponseBranches': explicit_json_returns, 'serverTypeCount': len(emitted),
            'serverVariantCount': len(emitted_variants), 'clientTypeCount': len(commands),
            'serializerKeySetsChecked': len(helpers), 'frontendCallSitesChecked': len(frontend_calls),
            'frontendCalls': frontend_calls, 'responseBranches': observed_responses,
            'sourceFiles': source_manifest}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--source-root', type=Path)
    parser.add_argument('--oas-schema', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    root = args.directory
    oas = read_json(root / 'meshchat-openapi.json')
    ws = read_json(root / 'meshchat-websocket.schema.json')
    protocol = read_json(root / 'meshchat-websocket.protocol.json')
    inventory = read_json(root / 'route-inventory.json')
    fixtures = read_json(root / 'contract-fixtures.json')
    assert yaml.safe_load((root / 'meshchat-openapi.yaml').read_text()) == oas, 'YAML/JSON mismatch'
    assert oas['x-source-commit'] == ws['x-source-commit'] == protocol['source']['commit'] == PIN
    refs_checked = 0
    for document in [oas, ws]:
        for node in walk(document):
            if isinstance(node, dict) and '$ref' in node:
                resolve_pointer(document, node['$ref'])
                refs_checked += 1
    Draft202012Validator.check_schema(ws)
    for schema in oas['components']['schemas'].values():
        Draft202012Validator.check_schema(schema)
    operation_ids = []
    for path, item in oas['paths'].items():
        for method, operation in item.items():
            if method not in HTTP_METHODS:
                continue
            operation_ids.append(operation['operationId'])
            expected = set(re.findall(r'\{([^}]+)\}', path))
            actual = {p['name'] for p in operation.get('parameters', []) if p['in'] == 'path'}
            assert expected == actual, (path, expected, actual)
            for node in walk(operation):
                if isinstance(node, dict) and 'schema' in node:
                    Draft202012Validator.check_schema(node['schema'])
                    if 'example' in node:
                        wrapper = {**node['schema'], 'components': oas['components']}
                        Draft202012Validator(wrapper).validate(node['example'])
    assert len(operation_ids) == len(set(operation_ids)) == 52
    results = []
    for category, document, prefix in [('rest', oas, '#/components/schemas/'), ('websocket', ws, '#/$defs/')]:
        for fixture in fixtures[category]:
            valid = validator(document, prefix + fixture['schema']).is_valid(fixture['payload'])
            assert valid == fixture['valid'], f"Fixture mismatch: {fixture['name']}"
            if category == 'websocket' and fixture['valid']:
                direction_schema = 'ClientMessage' if fixture['direction'] == 'client_to_server' else 'ServerMessage'
                validator(ws, '#/$defs/' + direction_schema).validate(fixture['payload'])
            results.append({'name': fixture['name'], 'category': category, 'expectedValid': fixture['valid'], 'passed': True})
    report = {'validatedAt': datetime.now(timezone.utc).isoformat(), 'sourceCommit': PIN,
              'status': 'passed', 'checks': {'yamlJsonEquivalent': True, 'localReferencesResolved': refs_checked,
              'openapiSchemaDefinitions': len(oas['components']['schemas']), 'websocketSchemaValid': True,
              'uniqueOperationIds': len(operation_ids), 'pathParametersMatch': True,
              'fixtureCount': len(results), 'fixtureResults': results},
              'officialOpenapiMetaSchema': {'status': 'not_run'}, 'sourceAudit': {'status': 'not_run'},
              'notPerformed': ['Starting MeshChat', 'Rust daemon integration', 'Network peer interoperability',
                               'Audio playback/codec verification', 'Exhaustive dependency-specific error testing']}
    if args.oas_schema:
        meta = read_json(args.oas_schema)
        Draft202012Validator(meta).validate(oas)
        report['officialOpenapiMetaSchema'] = {'status': 'passed', 'id': meta.get('$id'),
                'sha256': hashlib.sha256(args.oas_schema.read_bytes()).hexdigest()}
    if args.source_root:
        report['sourceAudit'] = source_checks(args.source_root, oas, ws, protocol, inventory)
    destination = args.report or root / 'validation-report.json'
    destination.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'status': 'passed', 'report': str(destination), 'fixtures': len(results),
                      'operations': len(operation_ids), 'sourceAudit': report['sourceAudit']['status'],
                      'officialOpenapiMetaSchema': report['officialOpenapiMetaSchema']['status']}))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (AssertionError, ValueError, KeyError, OSError) as error:
        print(f'VALIDATION FAILED: {error}', file=sys.stderr)
        raise SystemExit(1)
