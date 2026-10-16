#!/usr/bin/env python3
"""Exercise release signing with controlled Apple tool responses."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class NotarizationTests(unittest.TestCase):
    def run_gate(self, **overrides):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage = root / "release stage"
            stage.mkdir()
            (stage / "vapourfly").touch()
            (stage / "Vapourfly.app").mkdir()
            log = root / "calls"
            for tool in ["codesign", "ditto", "xcrun", "spctl"]:
                mock = root / tool
                mock.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
tool = Path(sys.argv[0]).name
with open(os.environ["CALL_LOG"], "a") as log:
    log.write(json.dumps([tool, *sys.argv[1:]]) + "\\n")
if tool == "codesign" and sys.argv[1] == "--display":
    print("TeamIdentifier=TESTTEAM", file=sys.stderr)
if tool == "codesign" and sys.argv[1] == "--verify":
    sys.exit(int(os.environ["VERIFY_EXIT"]))
if tool == "xcrun" and sys.argv[1] == "notarytool":
    print(json.dumps({"id": "test-submission", "status": os.environ["NOTARY_STATUS"],
                      "message": "Processing complete"}))
''')
                mock.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", CALL_LOG=str(log),
                       NOTARY_STATUS="Accepted", VERIFY_EXIT="0", APPLE_TEAM_ID="TESTTEAM",
                       APPLE_SIGNING_IDENTITY="Developer ID Application: Test (TESTTEAM)",
                       APPLE_SIGNING_KEYCHAIN="test.keychain-db", APPLE_API_KEY_PATH="test.p8",
                       APPLE_API_KEY_ID="KEYID", APPLE_API_ISSUER="issuer")
            env.update(overrides)
            result = subprocess.run(["bash", str(Path(__file__).with_name("sign-and-notarize.sh")),
                                     str(stage)], env=env, capture_output=True, text=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls

    def test_cli_and_app_are_signed_before_submission_and_app_is_stapled(self):
        result, calls = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        signed = [call for call in calls if call[:2] == ["codesign", "--force"]]
        self.assertEqual(len(signed), 2)
        self.assertTrue(signed[0][-1].endswith("/vapourfly"))
        self.assertTrue(signed[1][-1].endswith("/Vapourfly.app"))
        for call in signed:
            self.assertIn("--timestamp", call)
            self.assertEqual(call[call.index("--options") + 1], "runtime")
            self.assertEqual(call[call.index("--keychain") + 1], "test.keychain-db")
        commands = [call[:2] for call in calls]
        submit = commands.index(["xcrun", "notarytool"])
        self.assertLess(commands.index(["ditto", "-c"]), submit)
        self.assertEqual(commands[:submit].count(["codesign", "--verify"]), 2)
        self.assertLess(submit, commands.index(["xcrun", "stapler"]))
        self.assertEqual([call[2] for call in calls if call[:2] == ["xcrun", "stapler"]],
                         ["staple", "validate"])
        self.assertIn(["spctl", "--assess"], commands)
        self.assertTrue(any("--check-notarization" in call and "=notarized" in call
                            and call[-1].endswith("/vapourfly")
                            for call in calls))

    def test_rejected_submission_never_staples_or_assesses(self):
        result, calls = self.run_gate(NOTARY_STATUS="Invalid")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[:2] == ["xcrun", "stapler"] or call[0] == "spctl" for call in calls))

    def test_invalid_signature_never_submits(self):
        result, calls = self.run_gate(VERIFY_EXIT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[0] == "xcrun" for call in calls))

    def test_wrong_team_never_submits(self):
        result, calls = self.run_gate(APPLE_TEAM_ID="WRONGTEAM")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[0] == "xcrun" for call in calls))

    def test_ad_hoc_identity_never_signs(self):
        result, calls = self.run_gate(APPLE_SIGNING_IDENTITY="-")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_missing_api_key_never_signs(self):
        result, calls = self.run_gate(APPLE_API_KEY_PATH="")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
