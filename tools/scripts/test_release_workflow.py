"""Exercise release selection, literal refs and the publication dependency gate."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

from release_config import classify_tag, release_metadata, validate_needs

ROOT = Path(__file__).resolve().parents[2]


class ReleaseConfigurationTests(unittest.TestCase):
    def test_linux_candidate_selects_only_three_linux_architectures(self):
        metadata = release_metadata("v0.16.1-rc.1-linux", "0.16.1-rc.1-linux")
        self.assertEqual(metadata["prerelease"], "true")
        self.assertEqual(metadata["linux_only"], "true")
        targets = json.loads(metadata["build_matrix"])["include"]
        self.assertEqual([t["id"] for t in targets], ["linux-x86_64", "linux-aarch64", "linux-armv7"])
        self.assertTrue(all(t["os"].startswith("ubuntu-") for t in targets))
        packages = json.loads(metadata["linux_package_matrix"])["include"]
        self.assertEqual([p["build_id"] for p in packages], [t["id"] for t in targets])
        self.assertEqual([p["arch"] for p in packages], ["amd64", "arm64", "armhf"])

    def test_full_release_preserves_all_six_targets(self):
        metadata = release_metadata("v1.2.3", "1.2.3")
        self.assertEqual(metadata["prerelease"], "false")
        self.assertEqual(metadata["linux_only"], "false")
        self.assertEqual(len(json.loads(metadata["build_matrix"])["include"]), 6)

    def test_macos_release_jobs_use_supported_architecture_runners(self):
        metadata = release_metadata("v1.2.3", "1.2.3")
        targets = {target["id"]: target for target in json.loads(metadata["build_matrix"])["include"]}
        self.assertEqual(targets["macos-x86_64"]["os"], "macos-15-intel")
        self.assertEqual(targets["macos-aarch64"]["os"], "macos-15")
        release = (ROOT / ".github/workflows/release.yml").read_text()
        package = release.split("\n  package-macos:\n", 1)[1].split("\n  sbom:\n", 1)[0]
        self.assertIn("runs-on: macos-15\n", package)

    def test_every_supported_semver_prerelease_is_nonstable(self):
        for tag in ["v1.2.3-rc.1-linux", "1.2.3-preview.4", "v1.2.3-canary.7", "v1.2.3-0", "v1.2.3-linux", "v1.2.3--"]:
            with self.subTest(tag=tag):
                self.assertTrue(classify_tag(tag)["prerelease"] == "true")

    def test_invalid_and_shell_active_tags_are_rejected(self):
        for tag in ["v01.2.3", "v1.2", "v1.2.3-01", "v1.2.3-rc.01", "v1.2.3+build", "v1.2.3\n", "v1.2.3;echo bad", "v1.2.3$(id)"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                classify_tag(tag)

    def test_version_mismatch_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "VERSION"):
            release_metadata("v0.16.1-rc.1-linux", "0.16.0")

    def needs(self, linux_only=True):
        needs = {name: {"result": "success"} for name in [
            "prepare", "sdk-wrapper-parity", "build", "package-linux", "sbom", "container",
            "package-windows", "package-macos",
        ]}
        needs["prepare"]["outputs"] = {"linux_only": str(linux_only).lower()}
        if linux_only:
            for name in ["package-windows", "package-macos"]:
                needs[name]["result"] = "skipped"
        return needs

    def test_only_intentional_platform_skips_are_accepted(self):
        validate_needs(self.needs())
        validate_needs(self.needs(False))
        for linux_only in [True, False]:
            for name in self.needs(linux_only):
                expected = self.needs(linux_only)[name]["result"]
                for result in ["success", "skipped", "failure", "cancelled", "pending", ""]:
                    if result == expected:
                        continue
                    needs = self.needs(linux_only)
                    needs[name]["result"] = result
                    with self.subTest(linux_only=linux_only, name=name, result=result), self.assertRaises(ValueError):
                        validate_needs(needs)

    def test_missing_unknown_or_malformed_dependency_results_fail_closed(self):
        for name in self.needs():
            needs = self.needs()
            del needs[name]
            with self.subTest(missing=name), self.assertRaises(ValueError):
                validate_needs(needs)
        for value in [None, {}, [], "false", "", {"linux_only": True}]:
            needs = self.needs()
            needs["prepare"]["outputs"] = value
            with self.subTest(outputs=value), self.assertRaises(ValueError):
                validate_needs(needs)
        needs = self.needs()
        needs["surprise"] = {"result": "success"}
        with self.assertRaises(ValueError):
            validate_needs(needs)

    def test_prepare_reads_version_from_literal_tag_not_workflow_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "-q")
            git("config", "user.name", "Release Test")
            git("config", "user.email", "release@example.invalid")
            (root / "VERSION").write_text("1.2.3-rc.1-linux\n")
            git("add", "VERSION")
            git("commit", "-qm", "release source")
            source = git("rev-parse", "HEAD")
            git("tag", "v1.2.3-rc.1-linux")
            (root / "VERSION").write_text("9.9.9\n")
            result = subprocess.run(["python3", str(ROOT / "tools/scripts/release_config.py"), "prepare",
                                     "--tag", "v1.2.3-rc.1-linux", "--publish", "true"], cwd=root,
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            output = dict(line.split("=", 1) for line in result.stdout.splitlines())
            self.assertEqual(output["source_commit"], source)
            self.assertEqual(output["checkout_ref"], "v1.2.3-rc.1-linux")
            self.assertEqual(output["version"], "1.2.3-rc.1-linux")
            for tag in ["v9.9.9", "v1.2.3$(id)"]:
                result = subprocess.run(["python3", str(ROOT / "tools/scripts/release_config.py"), "prepare",
                                         "--tag", tag, "--publish", "true"], cwd=root,
                                        text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_unpublished_candidate_requires_exact_reachable_commit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "-q")
            git("config", "user.name", "Release Test")
            git("config", "user.email", "release@example.invalid")
            (root / "VERSION").write_text("1.2.3-rc.1\n")
            git("add", "VERSION")
            git("commit", "-qm", "candidate")
            source = git("rev-parse", "HEAD")
            command = ["python3", str(ROOT / "tools/scripts/release_config.py"),
                       "prepare", "--tag", "v1.2.3-rc.1", "--source-commit"]
            result = subprocess.run(command + [source, "--publish", "false"],
                                    cwd=root, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            output = dict(line.split("=", 1) for line in result.stdout.splitlines())
            self.assertEqual(output["checkout_ref"], source)
            self.assertEqual(output["publish"], "false")
            git("tag", "-a", "candidate", "-m", "candidate")
            tag_object = git("rev-parse", "refs/tags/candidate")
            unrelated = subprocess.check_output(
                ["git", "commit-tree", "HEAD^{tree}"], cwd=root, input="unrelated\n", text=True).strip()
            for commit, publish in [(source, "true"), ("HEAD", "false"),
                                    ("0" * 40, "false"), (tag_object, "false"),
                                    (unrelated, "false")]:
                result = subprocess.run(command + [commit, "--publish", publish],
                                        cwd=root, text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


class PromotionTests(unittest.TestCase):
    def test_only_approved_complete_source_matched_bytes_are_accepted(self):
        import hashlib
        from promote_candidate import validate_files
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = 'a' * 40
            (root / 'release-build-source.json').write_text(json.dumps(
                {'product_commit': source, 'release_tag': 'v1.2.3-rc.1'}))
            (root / 'package.tar.gz').write_bytes(b'qualified package')
            content = ''.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name + '\n'
                              for p in sorted(root.iterdir()))
            (root / 'SHA256SUMS.txt').write_text(content)
            digest = hashlib.sha256(content.encode()).hexdigest()
            self.assertEqual(len(validate_files(root, digest, 'v1.2.3-rc.1', source)), 3)
            with self.assertRaises(ValueError):
                validate_files(root, '0' * 64, 'v1.2.3-rc.1', source)
            with self.assertRaises(ValueError):
                validate_files(root, digest, 'v1.2.3-rc.1', 'b' * 40)
            (root / 'extra').write_bytes(b'unchecked')
            with self.assertRaises(ValueError):
                validate_files(root, digest, 'v1.2.3-rc.1', source)
            (root / 'extra').unlink()
            (root / 'package.tar.gz').write_bytes(b'changed')
            with self.assertRaises(ValueError):
                validate_files(root, digest, 'v1.2.3-rc.1', source)

    def test_promotion_uses_draft_id_and_verifies_assets_before_publication(self):
        from unittest.mock import patch
        import promote_candidate
        source = 'a' * 40
        env = {'RELEASE_TAG': 'v1.2.3-rc.1', 'SOURCE_COMMIT': source,
               'CHECKSUMS_SHA256': 'b' * 64, 'GH_REPO': 'test/repo',
               'RUN_ID': '42', 'GITHUB_SERVER_URL': 'https://github.com'}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'candidate'
            root.mkdir()
            (root / 'RELEASE_NOTES.md').write_text('Candidate')
            (root / 'release-build-source.json').write_text(json.dumps({
                'publish': False, 'workflow_commit': source,
                'workflow_run': 'https://github.com/test/repo/actions/runs/42'}))
            expected = {'package': 'sha256:digest'}
            draft = {'id': 123, 'draft': True, 'prerelease': True,
                     'tag_name': env['RELEASE_TAG'], 'target_commitish': source,
                     'assets': [{'name': 'package', 'digest': 'sha256:digest', 'state': 'uploaded'}]}
            outputs = [json.dumps({}), json.dumps(draft), json.dumps(draft),
                       json.dumps({'object': {'type': 'commit', 'sha': source}})]
            old = os.getcwd()
            try:
                os.chdir(temporary)
                with patch.dict(os.environ, env), patch.object(promote_candidate, 'validate_files', return_value=expected), \
                     patch.object(subprocess, 'check_output', side_effect=outputs) as read, \
                     patch.object(subprocess, 'run') as write:
                    promote_candidate.main()
                    self.assertIn('repos/test/repo/releases/123', read.call_args_list[2].args[0])
                    final = write.call_args_list[-1]
                    self.assertIn('PATCH', final.args[0])
                    self.assertIn('repos/test/repo/releases/123', final.args[0])
                    self.assertFalse(json.loads(final.kwargs['input'])['draft'])
            finally:
                os.chdir(old)


class ReleaseWorkflowWiringTests(unittest.TestCase):
    def test_stable_crate_publication_requires_a_successful_published_release(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        job = release.split("\n  publish-crates:\n", 1)[1].split("\n  homebrew:\n", 1)[0]
        self.assertIn("needs: [prepare, release]", job)
        self.assertIn("if: needs.prepare.outputs.publish == 'true' && needs.prepare.outputs.prerelease == 'false'", job)
        self.assertNotIn("always()", job)
        self.assertIn("actions: write", job)
        self.assertIn("GH_TOKEN: ${{ github.token }}", job)
        self.assertIn("RELEASE_TAG: ${{ needs.prepare.outputs.tag }}", job)

    def test_crate_dispatch_uses_the_tag_for_source_and_concurrency_and_propagates_failure(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        job = release.split("\n  publish-crates:\n", 1)[1].split("\n  homebrew:\n", 1)[0]
        script = textwrap.dedent(job.split("        run: |\n", 1)[1])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gh = root / "gh"
            gh.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                "with open(os.environ['GH_ARGUMENTS'], 'w') as destination:\n"
                "    json.dump(sys.argv[1:], destination)\n"
                "sys.exit(int(os.environ['GH_EXIT_CODE']))\n"
            )
            gh.chmod(0o755)
            arguments = root / "arguments.json"
            for exit_code in [0, 1]:
                with self.subTest(exit_code=exit_code):
                    env = dict(os.environ, PATH=f"{root}{os.pathsep}{os.environ['PATH']}",
                               GITHUB_REPOSITORY="FreeTAKTeam/LXMF-rs", RELEASE_TAG="v0.16.1",
                               GH_ARGUMENTS=str(arguments), GH_EXIT_CODE=str(exit_code))
                    result = subprocess.run(["bash", "-c", script], env=env,
                                            text=True, capture_output=True)
                    self.assertEqual(result.returncode, exit_code, result.stderr)
                    self.assertEqual(json.loads(arguments.read_text()), [
                        "workflow", "run", "crates-io-publish.yml",
                        "--repo", "FreeTAKTeam/LXMF-rs", "--ref", "v0.16.1",
                        "-f", "ref=v0.16.1", "-f", "version=v0.16.1", "-f", "dry_run=false",
                    ])

    def test_workflows_use_shared_policy_and_publish_only_after_guard(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("matrix: ${{ fromJSON(needs.prepare.outputs.build_matrix) }}", release)
        self.assertIn("matrix: ${{ fromJSON(needs.prepare.outputs.linux_package_matrix) }}", release)
        build = release.split("\n  build:\n", 1)[1].split("\n  package-linux:\n", 1)[0]
        self.assertIn("needs: [prepare, sdk-wrapper-parity]", build)
        self.assertNotIn("          - id:", build)
        for job in ["package-windows", "package-macos"]:
            block = release.split(f"\n  {job}:\n", 1)[1].split("\n  ", 1)[0]
            self.assertIn("if: needs.prepare.outputs.linux_only != 'true'", block)
        publish = release.split("\n  release:\n", 1)[1].split("\n  homebrew:\n", 1)[0]
        self.assertIn("always() && !cancelled()", publish)
        self.assertLess(publish.index("release_config.py check-needs"), publish.index("actions/download-artifact"))
        self.assertIn("RELEASE_NEEDS: ${{ toJSON(needs) }}", publish)
        self.assertIn('"$RELEASE_INPUT_TAG"', release)
        self.assertNotIn('tag="${{ inputs.version }}"', release)
        for workflow in ["release.yml", "performance-release.yml", "independent-interop.yml"]:
            text = (ROOT / ".github/workflows" / workflow).read_text()
            self.assertIn("release_config.py", text)
            self.assertIn("prerelease:", text)
            self.assertIn("make_latest:", text)
        independent = (ROOT / ".github/workflows/independent-interop.yml").read_text()
        attachment = independent.split("name: Attach standalone release evidence", 1)[1]
        self.assertIn("if: github.ref_type == 'tag'", attachment)
        self.assertIn("if: github.event_name != 'pull_request'", independent)


if __name__ == "__main__":
    unittest.main()
