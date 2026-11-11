"""Promote an already-qualified candidate without rebuilding its packages."""
import hashlib
import json
import os
from pathlib import Path
import subprocess


def validate_files(root, expected_checksum, tag, source):
    manifest = root / 'SHA256SUMS.txt'
    if hashlib.sha256(manifest.read_bytes()).hexdigest() != expected_checksum:
        raise ValueError('Checksum manifest differs from the qualified candidate')
    expected = {}
    for line in manifest.read_text().splitlines():
        digest, name = line.split('  ', 1)
        if Path(name).name != name or name in expected or len(digest) != 64:
            raise ValueError('Invalid or duplicate candidate filename')
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
            raise ValueError('Candidate checksum mismatch: ' + name)
        expected[name] = 'sha256:' + digest
    if {p.name for p in root.iterdir()} != set(expected) | {'SHA256SUMS.txt'}:
        raise ValueError('Candidate contains unchecked files')
    provenance = json.loads((root / 'release-build-source.json').read_text())
    if provenance['product_commit'] != source or provenance['release_tag'] != tag:
        raise ValueError('Candidate source mismatch')
    expected['SHA256SUMS.txt'] = 'sha256:' + expected_checksum
    return expected


def main():
    root = Path('candidate')
    tag, source = os.environ['RELEASE_TAG'], os.environ['SOURCE_COMMIT']
    expected = validate_files(root, os.environ['CHECKSUMS_SHA256'], tag, source)
    # The workflow token suppresses recursive tag/release publishing workflows.
    subprocess.run(['gh', 'release', 'create', tag, '--draft', '--prerelease',
                    '--latest=false', '--target', source, '--title', 'LXMF-rs ' + tag,
                    '--notes-file', str(root / 'RELEASE_NOTES.md')], check=True)
    subprocess.run(['gh', 'release', 'upload', tag, *map(str, sorted(root.iterdir()))], check=True)
    release = json.loads(subprocess.check_output([
        'gh', 'api', 'repos/' + os.environ['GH_REPO'] + '/releases/tags/' + tag]))
    actual = {a['name']: a['digest'] for a in release['assets']}
    if actual != expected or not release['draft']:
        raise ValueError('Draft assets differ from the verified candidate; leaving draft unpublished')
    subprocess.run(['gh', 'release', 'edit', tag, '--draft=false', '--prerelease', '--latest=false'], check=True)


if __name__ == '__main__':
    main()
