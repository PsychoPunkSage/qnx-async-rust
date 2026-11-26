#!/usr/bin/env python3
"""Plot a results CSV from run.sh: for each server (mio, tokio) and each
active-connection count, three panels (throughput, p99 latency, CPU per
request) comparing the poll() selector with the ionotify selector at each
number of open connections. Bars are medians of the runs; the label above
each pair is ionotify's improvement over poll().

Older CSVs label rows `poll` / `ionotify` (mio only); those are read as
`mio-poll` / `mio-ionotify`.

Usage: plot.py results/<stamp>.csv [out_dir]
"""

import csv
import statistics
import sys
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

SURFACE = "#ffffff"
TEXT = "#1f1f1e"
MUTED = "#6b6a66"
OLD = "#c4c2bc"  # poll(): the baseline, kept quiet
NEW = "#2a78d6"  # ionotify
# (column, title, higher_is_better, value format)
PANELS = [
    ("rps", "Requests per second", True, "{:,.0f}"),
    ("p99_us", "p99 latency (µs)", False, "{:,.0f}"),
    ("cpu_us_per_req", "CPU per request (µs)", False, "{:,.0f}"),
]


def load(path):
    cells = defaultdict(list)
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            label = row["label"]
            if label in ("poll", "ionotify"):
                label = f"mio-{label}"
            cells[(int(row["active"]), label, int(row["total"]))].append(row)
    return cells


def median(cells, key, column):
    return statistics.median(float(r[column]) for r in cells[key])


def plot(cells, server, active, out):
    totals = sorted({t for a, label, t in cells
                     if a == active and label.startswith(f"{server}-")})
    fig, axes = plt.subplots(1, len(PANELS), figsize=(12, 4.2), facecolor=SURFACE)
    width = 0.38
    for ax, (column, title, higher_better, fmt) in zip(axes, PANELS):
        old = [median(cells, (active, f"{server}-poll", t), column) for t in totals]
        new = [median(cells, (active, f"{server}-ionotify", t), column) for t in totals]
        top = max(old + new)
        xs = range(len(totals))
        ax.bar([x - width / 2 for x in xs], old, width, color=OLD, label="poll() (before)")
        ax.bar([x + width / 2 for x in xs], new, width, color=NEW, label="ionotify (after)")

        for x, o, n in zip(xs, old, new):
            for value, dx, color in ((o, -width / 2, MUTED), (n, width / 2, TEXT)):
                ax.text(x + dx, value + top * 0.02, fmt.format(value),
                        ha="center", va="bottom", fontsize=8, color=color)
            ratio = n / o if higher_better else o / n
            word = "faster" if column == "rps" else "lower"
            if ratio >= 1.05:
                label, color, weight = f"{ratio:.1f}× {word}", NEW, "bold"
            elif ratio > 1 / 1.05:
                label, color, weight = "same", MUTED, "normal"
            else:
                label, color, weight = f"{1 / ratio:.1f}× worse", MUTED, "normal"
            ax.text(x, top * 1.17, label, ha="center", va="bottom",
                    fontsize=10, color=color, fontweight=weight)

        ax.set_ylim(0, top * 1.32)
        ax.set_xticks(list(xs), [f"{t:,}" for t in totals])
        ax.set_xlabel("open connections", fontsize=9, color=MUTED)
        ax.set_title(title, loc="left", fontsize=11, color=TEXT, pad=10)
        ax.set_facecolor(SURFACE)
        ax.tick_params(axis="x", length=0, colors=TEXT, labelsize=10)
        ax.set_yticks([])
        for side in ("top", "right", "left"):
            ax.spines[side].set_visible(False)
        ax.spines["bottom"].set_color(OLD)

    handles, labels = axes[0].get_legend_handles_labels()
    fig.legend(handles, labels, loc="upper right", ncol=2, frameon=False,
               fontsize=10, labelcolor=TEXT, bbox_to_anchor=(0.99, 0.99))
    plural = "" if active == 1 else "s"
    fig.suptitle(f"{server} echo server on QNX 8, {active} active connection{plural}",
                 x=0.01, y=0.97, ha="left", fontsize=12, color=TEXT, fontweight="bold")
    fig.tight_layout(rect=(0, 0, 1, 0.92))
    fig.savefig(out, facecolor=SURFACE)
    plt.close(fig)


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2]) if len(sys.argv) == 3 else path.parent
    cells = load(path)
    for server in sorted({label.split("-")[0] for _, label, _ in cells}):
        for active in sorted({a for a, label, _ in cells if label.startswith(f"{server}-")}):
            out = out_dir / f"{path.stem}-{server}-active{active}.svg"
            plot(cells, server, active, out)
            print(f"wrote {out}")


if __name__ == "__main__":
    main()
