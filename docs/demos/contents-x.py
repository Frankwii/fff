#!/usr/bin/env python3
"""The keys of the contents recording, played into the X display that
record-x.sh starts (DISPLAY), as a tape plays them into VHS."""
import subprocess
import time


def run(*args):
    subprocess.run(["xdotool", *args], check=True)


def sleep(seconds):
    time.sleep(seconds)


def type_(text, delay=0.09):
    for c in text:
        run("type", "--delay", "0", c)
        time.sleep(delay)


def key(name, wait=0.3):
    run("key", name)
    time.sleep(wait)


type_("fff --depth 3"); sleep(0.4); key("Return", 1.8)
type_("i"); sleep(0.3); key("Tab", 0.6)
type_("Sobolev embedding", 0.12); sleep(2)
key("Escape"); type_("j"); sleep(1.2)
type_("j"); sleep(1.2); type_("k"); sleep(1.2)
# L opens the PDF at the match, beside fff; the terminal keeps the keyboard.
type_("L"); sleep(1.5)
subprocess.run(["i3-msg", "-q", "focus", "left"], check=True)
sleep(4)
