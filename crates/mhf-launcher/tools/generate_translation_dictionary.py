#!/usr/bin/env python3
"""Generate an MHF locale template from resource images."""

from __future__ import annotations

import argparse
import json
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


MAX_RECORD_SIZE = 64 * 1024


@dataclass(frozen=True)
class Group:
    key: str
    sources: tuple[str | None, ...]


def parse_locale(value: str) -> str:
    if not value or value in {".", ".."} or "/" in value or "\\" in value:
        raise argparse.ArgumentTypeError(
            "locale must be a non-empty file name without path separators"
        )
    return value


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Generate a locale JSONL template from decrypted, decompressed "
            "MHF resource images."
        )
    )
    parser.add_argument(
        "--locale",
        type=parse_locale,
        required=True,
        help="locale ID used as the output file name",
    )
    parser.add_argument("--dat", type=Path, required=True)
    parser.add_argument("--inf", type=Path, required=True)
    parser.add_argument("--pac", type=Path, required=True)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "translations",
    )
    return parser.parse_args()


def u16(data: bytes, offset: int) -> int:
    require_range(data, offset, 2)
    return struct.unpack_from("<H", data, offset)[0]


def u32(data: bytes, offset: int) -> int:
    require_range(data, offset, 4)
    return struct.unpack_from("<I", data, offset)[0]


def require_range(data: bytes, offset: int, size: int) -> None:
    if offset < 0 or size < 0 or offset + size > len(data):
        raise ValueError(
            f"range 0x{offset:X}..0x{offset + size:X} exceeds "
            f"the 0x{len(data):X}-byte image"
        )


def read_source(data: bytes, target: int) -> str | None:
    require_range(data, target, 1)
    end = data.find(b"\0", target, min(len(data), target + MAX_RECORD_SIZE))
    if end < 0:
        raise ValueError(f"string at 0x{target:X} has no bounded NUL terminator")
    raw = data[target:end]
    if not raw:
        return None
    try:
        # The client is a Windows ANSI application.  CP932 keeps ASCII 0x7E
        # as '~', which is significant because the game uses it in controls
        # such as ~C05; Python's shift_jisx0213 codec maps it to U+203E.
        return raw.decode("cp932")
    except UnicodeDecodeError as error:
        raise ValueError(f"string at 0x{target:X} is not Shift-JIS: {error}") from error


def record_group(
    resource: str,
    table_id: str,
    record_index: int,
    data: bytes,
    cells: tuple[int, ...],
) -> Group | None:
    sources: list[str | None] = []
    for cell in cells:
        target = u32(data, cell)
        if target == 0:
            sources.append(None)
            continue
        source = read_source(data, target)
        sources.append(source)
    if all(source is None for source in sources):
        return None
    return Group(
        key=f"{resource}:{table_id}:{record_index}",
        sources=tuple(sources),
    )


def add_cells(resource: str, data: bytes, seen_cells: set[int], cells: tuple[int, ...]) -> None:
    for cell in cells:
        require_range(data, cell, 4)
        if cell in seen_cells:
            raise ValueError(f"{resource} pointer cell 0x{cell:X} is described twice")
        seen_cells.add(cell)


def record_table_groups(
    resource: str,
    data: bytes,
    table: dict[str, Any],
    seen_cells: set[int],
) -> list[Group]:
    groups: list[Group] = []
    table_start = u32(data, table["root"])
    text_offset = table["text_offset"]
    stride = table["stride"]
    for record_index in range(table["records"]):
        record_start = table_start + record_index * stride
        cells = tuple(
            record_start + text_offset + part * 4
            for part in range(table["parts"])
        )
        add_cells(resource, data, seen_cells, cells)
        group = record_group(resource, table["id"], record_index, data, cells)
        if group is not None:
            groups.append(group)
    return groups


def quest_table_groups(
    resource: str,
    data: bytes,
    layout: dict[str, Any],
    seen_cells: set[int],
) -> list[Group]:
    category_table = u32(data, layout["root"])
    count_data = u32(data, layout["count_root"])
    category_count = u16(data, count_data)
    groups: list[Group] = []
    quest_ids: set[int] = set()
    for category_index in range(category_count):
        category = category_table + category_index * layout["category_stride"]
        record_count = u16(data, category + layout["category_count_field"])
        records = u32(data, category + layout["category_records_field"])
        if record_count and records == 0:
            raise ValueError(f"{resource} category {category_index} has records but no table")
        for record_index in range(record_count):
            record = u32(data, records + record_index * 4)
            if record == 0:
                continue
            quest_id = u16(data, record + layout["record_id_field"])
            if quest_id == 0:
                raise ValueError(
                    f"{resource} category {category_index} record {record_index} "
                    "uses reserved quest ID 0"
                )
            if quest_id in quest_ids:
                raise ValueError(f"{resource} quest ID {quest_id} is defined twice")
            quest_ids.add(quest_id)
            text_table = u32(data, record + layout["record_text_field"])
            if text_table == 0:
                continue
            cells = tuple(text_table + part * 4 for part in range(layout["parts"]))
            add_cells(resource, data, seen_cells, cells)
            group = record_group(resource, layout["id"], quest_id, data, cells)
            if group is not None:
                groups.append(group)
    return groups


def expand_layout(layout: dict[str, Any]) -> list[dict[str, Any]]:
    if layout.get("version") != 3:
        raise ValueError("unsupported resource layout version")
    record_layouts = layout["record_layouts"]
    resources: list[dict[str, Any]] = []
    for definition in layout["resources"]:
        resource = {
            "id": definition["id"],
            "magic": definition["identity"]["magic"],
            "format_version": definition["identity"]["format_version"],
            "type": definition["type"],
        }
        if definition["type"] == "records":
            tables: list[dict[str, Any]] = []
            for table in definition["tables"]:
                tables.append(
                    expand_record_table(record_layouts, table, table["root_field"])
                )
            for directory in definition["table_directories"]:
                first_root = directory["first_root_field"]
                root_stride = directory["root_stride"]
                for index, entry in enumerate(directory["entries"]):
                    table = {
                        "id": entry["id"],
                        "records": entry["records"],
                        "layout": directory["layout"],
                    }
                    tables.append(
                        expand_record_table(
                            record_layouts,
                            table,
                            first_root + index * root_stride,
                        )
                    )
            tables.sort(key=lambda table: table["root"])
            resource["tables"] = tables
        elif definition["type"] == "quest":
            quest = definition["layout"]
            resource["layout"] = {
                "id": quest["id"],
                "root": quest["root_field"],
                "count_root": quest["count_root_field"],
                "category_stride": quest["category_stride"],
                "category_count_field": quest["category_count_field"],
                "category_records_field": quest["category_records_field"],
                "record_text_field": quest["record_text_field"],
                "record_id_field": quest["record_id_field"],
                "parts": quest["parts"],
            }
        else:
            raise ValueError(
                f"{definition['id']} has unknown resource type {definition['type']!r}"
            )
        resources.append(resource)
    return resources


def expand_record_table(
    record_layouts: dict[str, Any],
    table: dict[str, Any],
    root: int,
) -> dict[str, Any]:
    layout_id = table["layout"]
    try:
        record_layout = record_layouts[layout_id]
    except KeyError as error:
        raise ValueError(f"unknown record layout {layout_id!r}") from error
    return {
        "id": table["id"],
        "root": root,
        "records": table["records"],
        "text_offset": record_layout["text_offset"],
        "parts": record_layout["parts"],
        "stride": record_layout["stride"],
    }


def extract(resource: dict[str, Any], data: bytes) -> list[Group]:
    if u32(data, 0) != resource["magic"]:
        raise ValueError(f"{resource['id']} has the wrong magic")
    if u32(data, 4) != resource["format_version"]:
        raise ValueError(f"{resource['id']} has an unsupported format version")
    groups: list[Group] = []
    seen_cells: set[int] = set()
    if resource["type"] == "records":
        for table in resource["tables"]:
            groups.extend(record_table_groups(resource["id"], data, table, seen_cells))
    elif resource["type"] == "quest":
        groups.extend(
            quest_table_groups(resource["id"], data, resource["layout"], seen_cells)
        )
    else:
        raise ValueError(f"{resource['id']} has unknown resource type")
    return groups


def json_line(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def key_identity(value: str) -> tuple[str, ...]:
    components = value.split(":")
    if (
        len(components) == 3
        and components[0] in {"mhfdat", "mhfinf", "mhfpac"}
        and components[2]
        and components[2].isascii()
        and components[2].isdigit()
    ):
        return components[0], components[1], str(int(components[2], 10))
    if len(components) == 4 and components[0] == "stage":
        stage = components[1]
        section = components[2].removeprefix("0x").removeprefix("0X")
        record = components[3].removeprefix("0x").removeprefix("0X")
        if (
            stage
            and stage.isascii()
            and stage.isdigit()
            and section
            and section.isascii()
            and all(character in "0123456789abcdefABCDEF" for character in section)
            and record
            and record.isascii()
            and all(character in "0123456789abcdefABCDEF" for character in record)
        ):
            return (
                "stage",
                str(int(stage, 10)),
                str(int(section, 16)),
                str(int(record, 16)),
            )
    return "raw", value


def read_existing(path: Path) -> dict[tuple[str, ...], dict[str, Any]]:
    if not path.exists():
        return {}

    records: dict[tuple[str, ...], dict[str, Any]] = {}
    with path.open(encoding="utf-8") as lines:
        for line_number, line in enumerate(lines, 1):
            if not line.strip():
                continue
            record = json.loads(line)
            if not isinstance(record, dict) or not isinstance(record.get("key"), str):
                raise ValueError(
                    f"{path}:{line_number} must contain an object with a string key"
                )
            identity = key_identity(record["key"])
            if identity in records:
                raise ValueError(
                    f"{path}:{line_number} defines duplicate key {record['key']!r}"
                )
            records[identity] = record
    return records


def write_output(output_dir: Path, locale: str, groups: list[Group]) -> None:
    target = output_dir / f"{locale}.jsonl"
    existing = read_existing(target)
    generated_keys: set[tuple[str, ...]] = set()
    records: list[dict[str, Any]] = []
    for group in groups:
        identity = key_identity(group.key)
        generated_keys.add(identity)
        record = {
            "key": group.key,
            "source": (
                group.sources[0]
                if len(group.sources) == 1
                else list(group.sources)
            ),
        }
        for field, value in existing.get(identity, {}).items():
            if field not in {"key", "source"}:
                record[field] = value
        records.append(record)

    extra_records = [
        record for identity, record in existing.items() if identity not in generated_keys
    ]
    records.extend(extra_records)
    output_dir.mkdir(parents=True, exist_ok=True)
    contents = "\n".join(json_line(record) for record in records)
    target.write_text(f"{contents}\n" if contents else "", encoding="utf-8")
    print(
        f"wrote {len(groups)} resource groups and preserved "
        f"{len(extra_records)} additional groups in {target}"
    )


def main() -> None:
    args = parse_args()
    layout_path = Path(__file__).resolve().parents[1] / "translations" / "resources.json"
    layout = json.loads(layout_path.read_text(encoding="utf-8"))
    paths = {"mhfdat": args.dat, "mhfinf": args.inf, "mhfpac": args.pac}
    all_groups: list[Group] = []
    for resource in expand_layout(layout):
        path = paths[resource["id"]]
        data = path.read_bytes()
        groups = extract(resource, data)
        print(f"{resource['id']}: {len(groups)} non-empty logical groups")
        all_groups.extend(groups)
    write_output(args.output_dir, args.locale, all_groups)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
