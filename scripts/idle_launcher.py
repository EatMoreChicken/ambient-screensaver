#!/usr/bin/env python3
"""Launch the development build when the current GNOME session becomes idle."""

import argparse
import math
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path


IDLE_CALL = (
    "gdbus",
    "call",
    "--session",
    "--dest",
    "org.gnome.Mutter.IdleMonitor",
    "--object-path",
    "/org/gnome/Mutter/IdleMonitor/Core",
    "--method",
    "org.gnome.Mutter.IdleMonitor.GetIdletime",
)
LOCK_CALL = (
    "gdbus",
    "call",
    "--session",
    "--dest",
    "org.gnome.ScreenSaver",
    "--object-path",
    "/org/gnome/ScreenSaver",
    "--method",
    "org.gnome.ScreenSaver.GetActive",
)


def call_gnome(command):
    return subprocess.check_output(command, text=True, stderr=subprocess.PIPE, timeout=5).strip()


def idle_milliseconds():
    result = call_gnome(IDLE_CALL)
    match = re.fullmatch(r"\(uint64 (\d+),\)", result)
    if not match:
        raise ValueError(f"unexpected GNOME idle response: {result}")
    return int(match.group(1))


def screen_locked():
    result = call_gnome(LOCK_CALL)
    if result not in ("(true,)", "(false,)"):
        raise ValueError(f"unexpected GNOME lock response: {result}")
    return result == "(true,)"


class IdleGate:
    """Allow one launch per idle period, even if the display exits early."""

    def __init__(self, threshold_ms):
        self.threshold_ms = threshold_ms
        self.armed = True

    def should_launch(self, idle_ms, locked):
        if idle_ms < self.threshold_ms:
            self.armed = True
        if self.armed and idle_ms >= self.threshold_ms and not locked:
            self.armed = False
            return True
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--idle-seconds", type=float, default=120)
    parser.add_argument("--poll-seconds", type=float, default=2)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--check", action="store_true", help="check GNOME and exit")
    parser.add_argument("photos", nargs="*", type=Path)
    args = parser.parse_args()
    if (
        not math.isfinite(args.idle_seconds)
        or not math.isfinite(args.poll_seconds)
        or args.idle_seconds <= 0
        or args.poll_seconds <= 0
    ):
        parser.error("idle and poll intervals must be positive")
    if not args.check and not args.photos:
        parser.error("provide at least one photo directory")
    if not args.check:
        for path in args.photos:
            if not path.expanduser().exists():
                parser.error(f"photo path does not exist: {path}")

    repo = Path(__file__).resolve().parent.parent
    binary = args.binary or repo / "target/release/ambient-screensaver"
    binary = binary.expanduser().resolve()
    if not args.check and (not binary.is_file() or not os.access(binary, os.X_OK)):
        parser.error(f"{binary} is missing or not executable; run cargo build --release first")
    photos = [str(path.expanduser().resolve()) for path in args.photos]

    try:
        idle_ms = idle_milliseconds()
        locked = screen_locked()
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        parser.error(f"GNOME idle/lock services are unavailable: {error}")
    if args.check:
        print(f"GNOME idle: {idle_ms / 1000:.1f}s; screen locked: {locked}")
        return

    gate = IdleGate(round(args.idle_seconds * 1000))
    child = None
    print(f"Waiting for {args.idle_seconds:g}s of GNOME inactivity", flush=True)

    def stop_on_termination(_signum, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop_on_termination)
    try:
        while True:
            try:
                idle_ms = idle_milliseconds()
                locked = screen_locked()
            except (OSError, subprocess.SubprocessError, ValueError) as error:
                print(f"GNOME idle check failed: {error}", file=sys.stderr)
                time.sleep(args.poll_seconds)
                continue

            if gate.should_launch(idle_ms, locked):
                print(f"Starting Ambient Photos after {idle_ms / 1000:.0f}s idle", flush=True)
                child = subprocess.Popen([str(binary), *photos])
                while child.poll() is None:
                    try:
                        if screen_locked():
                            child.terminate()
                            try:
                                child.wait(timeout=5)
                            except subprocess.TimeoutExpired:
                                child.kill()
                                child.wait()
                            break
                    except (OSError, subprocess.SubprocessError, ValueError) as error:
                        print(f"GNOME lock check failed: {error}", file=sys.stderr)
                    time.sleep(args.poll_seconds)
                child = None
                print("Display closed; waiting for new user activity", flush=True)
            time.sleep(args.poll_seconds)
    except KeyboardInterrupt:
        pass
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


if __name__ == "__main__":
    main()
