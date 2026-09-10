import contextlib
import io
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

import generate_translation_dictionary as generator


class ResourceExtractionTests(unittest.TestCase):
    def image(self):
        data = bytearray(128)
        struct.pack_into("<I", data, 0, 32)
        struct.pack_into("<II", data, 32, 64, 2)
        struct.pack_into("<II", data, 64, 96, 104)
        data[96:99] = "日".encode("cp932") + b"\0"
        data[104:107] = "本".encode("cp932") + b"\0"
        return data

    def table(self, **fields):
        return dict(
            id="sample", root=[0, 0], records={"u32_at": [0, 4]},
            stride=4, text_offset=0, parts=1, **fields,
        )

    def test_nested_roots_and_counts_extract_only_the_declared_records(self):
        data = self.image()
        groups = generator.record_table_groups("mhfgao", data, self.table(), set())
        self.assertEqual(
            groups,
            [generator.Group("mhfgao:sample:0", ("日",)),
             generator.Group("mhfgao:sample:1", ("本",))],
        )
        struct.pack_into("<H", data, 12, 1)
        table = self.table()
        table["records"] = {"u16_at": 12}
        self.assertEqual(
            generator.record_table_groups("mhfgao", data, table, set()), groups[:1]
        )

    def test_null_roots_and_null_child_tables_are_not_header_text(self):
        for field in (0, 32):
            data = self.image()
            struct.pack_into("<I", data, field, 0)
            self.assertEqual(
                generator.record_table_groups("mhfgao", data, self.table(), set()), []
            )
        data = self.image()
        struct.pack_into("<I", data, 64, 0)
        groups = generator.record_table_groups("mhfgao", data, self.table(), set())
        self.assertEqual(groups, [generator.Group("mhfgao:sample:1", ("本",))])

    def test_directory_count_skips_inactive_entries_before_dereferencing(self):
        data = self.image()
        struct.pack_into("<I", data, 8, 1)
        struct.pack_into("<II", data, 40, 0xFFFFFFFF, 2)
        layout = dict(version=4, record_layouts={
            "text": dict(stride=4, text_offset=0, parts=1)
        }, resources=[dict(id="mhfgao", type="records", tables=[
            dict(id=f"location_{index:02}", root_field=[0, index * 8],
                 records={"u32_at": [0, index * 8 + 4]}, layout="text",
                 directory=dict(index=index, count={"u32_at": 8}))
            for index in range(2)
        ])])
        resource = generator.expand_layout(layout)[0]
        self.assertEqual(generator.extract(resource, data), [
            generator.Group("mhfgao:location_00:0", ("日",)),
            generator.Group("mhfgao:location_00:1", ("本",)),
        ])
        struct.pack_into("<I", data, 8, 0)
        struct.pack_into("<I", data, 0, 0xFFFFFFFF)
        self.assertEqual(generator.extract(resource, data), [])

    def test_count_can_follow_a_separate_metadata_sentinel(self):
        data = self.image()
        struct.pack_into("<I", data, 4, 80)
        struct.pack_into("<HHH", data, 80, 12, 34, 65535)
        count = {"until": dict(root_field=4, stride=2, offset=0, width=2, value=65535)}
        table = self.table()
        table["records"] = count
        self.assertEqual(len(generator.record_table_groups("mhfgao", data, table, set())), 2)
        table["records"] = {
            "until": dict(root_field=[0, 0], stride=4, offset=0, width=4, value=0)
        }
        self.assertEqual(len(generator.record_table_groups("mhfgao", data, table, set())), 2)

    def test_unterminated_sentinel_and_overrun_count_fail_at_the_image_boundary(self):
        data = self.image()
        struct.pack_into("<I", data, 4, 124)
        struct.pack_into("<I", data, 124, 1)
        count = {"until": dict(root_field=4, stride=4, offset=0, width=4, value=0)}
        with self.assertRaisesRegex(ValueError, "exceeds"):
            generator.record_count(data, count)
        count["until"]["stride"] = 0
        with self.assertRaisesRegex(ValueError, "sentinel"):
            generator.record_count(data, count)
        table = self.table()
        table["records"] = 1000
        with self.assertRaisesRegex(ValueError, "exceeds"):
            generator.record_table_groups("mhfgao", data, table, set())

    def test_v4_mixes_direct_roots_paths_and_optional_identity(self):
        definition = dict(id="mhfgao", type="records", tables=[
            dict(id="nested", root_field=[0, 0], records={"u32_at": [0, 4]}, layout="text")
        ], table_directories=[
            dict(first_root_field=8, root_stride=4, layout="text", entries=[
                dict(id="empty", records=1)
            ])
        ])
        layout = dict(version=4, record_layouts={
            "text": dict(stride=4, text_offset=0, parts=1)
        }, resources=[definition])
        resource = generator.expand_layout(layout)[0]
        self.assertEqual(len(generator.extract(resource, self.image())), 2)
        definition["identity"] = dict(magic=123, format_version=4)
        with self.assertRaisesRegex(ValueError, "magic"):
            generator.extract(generator.expand_layout(layout)[0], self.image())

    def test_shared_pointer_table_segments_have_independent_record_ids(self):
        data = self.image()
        struct.pack_into("<IIII", data, 64, 96, 104, 96, 104)
        definition = dict(id="mhfdat", type="records", tables=[
            dict(id="item_descriptions", root_field=[0, 0], first_record=2,
                 records=2, layout="text"),
            dict(id="item_messages", root_field=[0, 0], records=2, layout="text"),
        ])
        layout = dict(version=4, record_layouts={
            "text": dict(stride=4, text_offset=0, parts=1)
        }, resources=[definition])
        resource = generator.expand_layout(layout)[0]
        self.assertEqual(generator.extract(resource, data), [
            generator.Group("mhfdat:item_messages:0", ("日",)),
            generator.Group("mhfdat:item_messages:1", ("本",)),
            generator.Group("mhfdat:item_descriptions:0", ("日",)),
            generator.Group("mhfdat:item_descriptions:1", ("本",)),
        ])
        definition["tables"][0]["first_record"] = 1
        with self.assertRaisesRegex(ValueError, "described twice"):
            generator.extract(generator.expand_layout(layout)[0], data)

    def test_partial_refresh_preserves_other_resources_and_translation_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "test.jsonl"
            rows = [dict(key="mhfgao:sample:0001", source="old", translation="译文", note="keep"),
                    dict(key="mhfjmp:sample:0", source="other", translation="其他"),
                    dict(key="stage:001:0010:0002", translation="stage")]
            target.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()):
                generator.write_output(Path(directory), "test", [
                    generator.Group("mhfgao:sample:1", ("new",))
                ])
            result = [json.loads(line) for line in target.read_text().splitlines()]
            self.assertEqual(result[0], dict(key="mhfgao:sample:1", source="new", translation="译文", note="keep"))
            self.assertEqual(result[1:], rows[1:])
        for name in generator.RESOURCE_NAMES:
            self.assertEqual(generator.key_identity(f"mhf{name}:sample:0001"), (f"mhf{name}", "sample", "1"))

    def test_cli_requires_one_image_and_accepts_each_resource_independently(self):
        with patch.object(sys, "argv", ["generate", "--locale", "test"]):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                generator.parse_args()
        for name in generator.RESOURCE_NAMES:
            with patch.object(sys, "argv", ["generate", "--locale", "test", f"--{name}", "resource.bin"]):
                self.assertEqual(getattr(generator.parse_args(), name), Path("resource.bin"))


if __name__ == "__main__":
    unittest.main()
