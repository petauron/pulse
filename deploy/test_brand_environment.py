import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("migration", Path(__file__).with_name("migrate-environment.py"))
migration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)


class EnvironmentMigrationTests(unittest.TestCase):
    def test_preserves_all_values_and_private_original(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, target = root / "old.env", root / "new.env"
            raw = b'# keep comment\nPULSE_NODE_REGION="JP"\nPULSE_GEOIP_PROVIDER=disabled\nPULSE_CREDENTIALS_PATH=/existing/private/identity.json\nPULSE_TRAFFIC_KEY_FILE=/existing/private/key\nPULSE_INTERVAL_SECONDS=30\n'
            source.write_bytes(raw)
            source.chmod(0o600)
            migration.migrate(source, target)
            self.assertEqual(source.read_bytes(), raw)
            self.assertEqual(target.read_bytes(), raw.replace(b"PULSE_", b"SENULUN_"))
            self.assertEqual(target.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                migration.migrate(source, target)

    def test_conflicts_and_shell_directives_are_not_executed(self):
        for raw in [b'PULSE_NODE_REGION=JP\nSENULUN_NODE_REGION=US\n', b'export PULSE_NODE_REGION=JP\n', b'source /private/file\n']:
            with self.assertRaises(ValueError):
                migration.convert(raw)


if __name__ == "__main__":
    unittest.main()
