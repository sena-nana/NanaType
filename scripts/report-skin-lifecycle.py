"""Summarize the retained multi-output GPU lifecycle probe (CPU milliseconds)."""
import argparse
import json
from pathlib import Path


def distribution(values):
    values = sorted(values)
    return {
        "samples": len(values),
        "median_ms": values[len(values) // 2],
        "p95_ms": values[len(values) * 95 // 100],
        "max_ms": values[-1],
    }


def summarize(path):
    report = json.loads(path.read_text(encoding="utf-8"))
    warm = [detail for row in report["switches"][1:] for detail in row["details"]]
    return {
        "source": str(path),
        "adapters": report["adapters"],
        "warm_switch_record_submit": distribution([d["record_ms"] for d in warm]),
        "last_round_steady": report["switches"][-1]["steady"],
        "last_round_steady_cpu_and_gpu_wait": report["switches"][-1].get("steady_cpu_and_gpu_wait"),
        "hot_reload": report["hot_reload"],
        "two_devices_warm": report["two_devices_warm"],
        "two_devices_after_240_frames": report["two_devices_after_240_frames"],
        "after_resize": report["after_resize"],
        "after_close": report["after_close"],
        "output_isolation_pixels": report["output_isolation_pixels"],
        "note": report["note"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--dx12", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = {"before_vulkan": summarize(args.before), "after_vulkan": summarize(args.after)}
    if args.dx12:
        result["after_dx12"] = summarize(args.dx12)
    args.out.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
