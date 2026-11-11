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
    def api(path, payload=None):
        command = ['gh', 'api', 'repos/' + os.environ['GH_REPO'] + '/' + path]
        if payload is not None:
            command += ['--method', 'POST', '--input', '-']
        return json.loads(subprocess.check_output(command, input=json.dumps(payload) if payload is not None else None, text=True))
    provenance = json.loads((root / 'release-build-source.json').read_text())
    expected_run = os.environ['GITHUB_SERVER_URL'] + '/' + os.environ['GH_REPO'] + '/actions/runs/' + os.environ['RUN_ID']
    if provenance.get('publish') is not False or provenance['workflow_commit'] != source or provenance['workflow_run'] != expected_run:
        raise ValueError('Candidate provenance does not match the approved unpublished run')
    # A workflow token suppresses recursive publishers; creating a new ref rejects existing tags.
    api('git/refs', {'ref': 'refs/tags/' + tag, 'sha': source})
    release = api('releases', {'tag_name': tag, 'target_commitish': source,
                              'name': 'LXMF-rs ' + tag, 'draft': True, 'prerelease': True,
                              'make_latest': 'false', 'body': (root / 'RELEASE_NOTES.md').read_text()})
    release_id = release['id']
    if not release['draft'] or not release['prerelease'] or release['tag_name'] != tag:
        raise ValueError('Unexpected draft response')
    subprocess.run(['gh', 'release', 'upload', tag, *map(str, sorted(root.iterdir()))], check=True)
    release = api('releases/' + str(release_id))
    actual = {a['name']: a['digest'] for a in release['assets']}
    if (actual != expected or not release['draft'] or not release['prerelease']
            or release['tag_name'] != tag or release['target_commitish'] != source
            or any(a['state'] != 'uploaded' for a in release['assets'])):
        raise ValueError('Draft assets differ from the verified candidate; leaving draft unpublished')
    ref = api('git/ref/tags/' + tag)
    if ref['object']['type'] != 'commit' or ref['object']['sha'] != source:
        raise ValueError('Release tag moved; leaving draft unpublished')
    subprocess.run(['gh', 'api', '--method', 'PATCH',
                    'repos/' + os.environ['GH_REPO'] + '/releases/' + str(release_id),
                    '--input', '-'], input=json.dumps({'draft': False, 'prerelease': True, 'make_latest': 'false'}), text=True, check=True)


if __name__ == '__main__':
    main()
