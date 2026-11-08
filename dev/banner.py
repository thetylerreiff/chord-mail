#!/usr/bin/env python3
"""Draws the README banner (docs/banner.png): the CHORD wordmark beside an
exploded isometric drawing of what one keypress does. The key lands on the
local cache at once, and the cache syncs to Gmail behind it.

Usage: dev/banner.py [out.png]   (needs Pillow; uses JetBrains Mono)
"""

import math
import sys

from PIL import Image, ImageDraw, ImageFont

OUT = sys.argv[1] if len(sys.argv) > 1 else "docs/banner.png"
W, H = 2400, 800
SS = 2  # supersampling, for smooth edges
MONO = "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf"
MONO_BOLD = "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Bold.ttf"

BG = (10, 10, 11)
DOT = (26, 27, 31)
HAIR = (52, 54, 60)
DIM = (118, 120, 128)
INK = (233, 232, 227)
SIGNAL = (255, 91, 31)
SIGNAL_MID = (226, 76, 22)
SIGNAL_LOW = (176, 56, 14)
FACE_TOP = (24, 25, 29)
FACE_L = (17, 18, 21)
FACE_R = (13, 13, 16)

img = Image.new("RGB", (W * SS, H * SS), BG)
d = ImageDraw.Draw(img)
COS30 = math.cos(math.radians(30))


def P(v):
    return round(v * SS)


def font(size, bold=False):
    return ImageFont.truetype(MONO_BOLD if bold else MONO, P(size))


def text(x, y, s, size=17, fill=DIM, bold=False, anchor="ls", track=0.14):
    """Mono text with wide tracking, like a label on a panel."""
    f = font(size, bold)
    adv = [d.textlength(c, font=f) + P(size * track) for c in s]
    width = sum(adv) - P(size * track)
    cx = P(x) - (width if anchor[0] == "r" else 0)
    for c, a in zip(s, adv):
        d.text((cx, P(y)), c, font=f, fill=fill, anchor="l" + anchor[1])
        cx += a


def line(points, fill=HAIR, width=1.5):
    d.line([(P(x), P(y)) for x, y in points], fill=fill, width=P(width))


def poly(points, fill=None, outline=None, width=1.5):
    pts = [(P(x), P(y)) for x, y in points]
    d.polygon(pts, fill=fill)
    if outline:
        d.line(pts + [pts[0]], fill=outline, width=P(width), joint="curve")


def iso(cx, cy, u, v, z=0.0):
    """A point on the isometric plane through (cx, cy): u runs down-right,
    v down-left, z straight up."""
    return (cx + (u - v) * COS30, cy + (u + v) * 0.5 - z)


# --- Ground: a quiet dot grid, crop marks, two hairlines ------------------

for gx in range(40, W, 40):
    for gy in range(40, H, 40):
        d.ellipse((P(gx - 1.3), P(gy - 1.3), P(gx + 1.3), P(gy + 1.3)), fill=DOT)

M = 48
for sx, sy in [(1, 1), (-1, 1), (1, -1), (-1, -1)]:
    cx, cy = (M if sx > 0 else W - M), (M if sy > 0 else H - M)
    line([(cx, cy), (cx + 22 * sx, cy)], DIM, 1.5)
    line([(cx, cy), (cx, cy + 22 * sy)], DIM, 1.5)

TOP, BOT = 112, H - 112
LEFT, RIGHT = 136, W - 136
line([(LEFT, TOP), (RIGHT, TOP)])
line([(LEFT, BOT), (RIGHT, BOT)])
for tx in range(LEFT, RIGHT + 1, 96):
    line([(tx, TOP), (tx, TOP + 8)])
text(LEFT, 80, "CHORD/MAIL", 17, INK, bold=True)
text(LEFT + 200, 80, "0.1.0", 17)
text(RIGHT, 80, "KEYBOARD-DRIVEN GMAIL FOR OMARCHY", 17, anchor="rs")
text(LEFT, H - 64, "LINUX  X86_64 / AARCH64", 17)
text(RIGHT, H - 64, "FIG. 1  ONE KEYPRESS, EXPLODED", 17, anchor="rs")

# --- Wordmark: drawn, not typeset ------------------------------------------

LH, T, LW, GAP = 150, 26, 150, 40
C = 26  # chamfer


def chamfer(x0, y0, x1, y1, c, corners="tl tr br bl"):
    tl, tr, br, bl = (c if k in corners.split() else 0 for k in ("tl", "tr", "br", "bl"))
    return [(x0 + tl, y0), (x1 - tr, y0), (x1, y0 + tr), (x1, y1 - br),
            (x1 - br, y1), (x0 + bl, y1), (x0, y1 - bl), (x0, y0 + tl)]


def rect(x0, y0, x1, y1, fill):
    d.rectangle((P(x0), P(y0), P(x1) - 1, P(y1) - 1), fill=fill)


def letter_c(x, y):
    poly(chamfer(x, y, x + LW, y + LH, C), INK)
    poly(chamfer(x + T, y + T, x + LW + 2, y + LH - T, 8, "tl bl"), BG)


def letter_h(x, y):
    rect(x, y, x + T, y + LH, INK)
    rect(x + LW - T, y, x + LW, y + LH, INK)
    rect(x, y + (LH - T) / 2, x + LW, y + (LH + T) / 2, INK)


def letter_o(x, y):
    poly(chamfer(x, y, x + LW, y + LH, C), INK)
    poly(chamfer(x + T, y + T, x + LW - T, y + LH - T, 8), BG)


def letter_r(x, y):
    mid = y + LH * 0.55
    poly(chamfer(x, y, x + LW, mid + T / 2, C, "tr br"), INK)
    poly(chamfer(x + T, y + T, x + LW - T, mid - T / 2, 8, "tr br"), BG)
    rect(x, y, x + T, y + LH, INK)
    lean = T * 1.2
    poly([(x + LW * 0.38, mid), (x + LW * 0.38 + lean, mid), (x + LW, y + LH), (x + LW - lean, y + LH)], INK)


def letter_d(x, y):
    poly(chamfer(x, y, x + LW, y + LH, C * 2, "tr br"), INK)
    poly(chamfer(x + T, y + T, x + LW - T, y + LH - T, C * 2 - T * 0.6, "tr br"), BG)


wx, wy = LEFT, H / 2 - LH / 2 - 18
for i, draw_letter in enumerate([letter_c, letter_h, letter_o, letter_r, letter_d]):
    draw_letter(wx + i * (LW + GAP), wy)
word_end = wx + 5 * LW + 4 * GAP
text(wx, wy + LH + 62, "MAIL AT THE SPEED OF A KEYPRESS.", 22, INK, track=0.1)
text(wx, wy + LH + 98, "LOCAL-FIRST  ·  NEOVIM COMPOSE  ·  OPEN SOURCE", 17, track=0.14)

# --- The exploded stack ------------------------------------------------------

SX = 1560  # stack axis
A = 210  # plate size
PT = 16  # plate thickness


def slab(cy, a, t, top, left, right, edge=DIM):
    """A square plate whose top face is centred on (SX, cy)."""
    h2 = a / 2
    tl, tr, br, bl = (iso(SX, cy, -h2, -h2), iso(SX, cy, h2, -h2),
                      iso(SX, cy, h2, h2), iso(SX, cy, -h2, h2))
    down = lambda p: (p[0], p[1] + t)
    poly([bl, br, down(br), down(bl)], left, edge)
    poly([br, tr, down(tr), down(br)], right, edge)
    poly([tl, tr, br, bl], top, edge)
    return tr  # the right-hand corner, for a callout


def dashed(x, y0, y1, dash=7, gap=7, fill=DIM):
    y = y0
    while y < y1:
        line([(x, y), (x, min(y + dash, y1))], fill, 1.5)
        y += dash + gap


GMAIL_Y, CACHE_Y, KEY_BASE_Y = 560, 412, 236

# 03 Gmail: a plate with an envelope etched into it.
gmail_corner = slab(GMAIL_Y, A, PT, FACE_TOP, FACE_L, FACE_R)
eu, ev = A * 0.30, A * 0.20
env = [iso(SX, GMAIL_Y, u, v) for u, v in [(-eu, -ev), (eu, -ev), (eu, ev), (-eu, ev)]]
poly(env, None, INK, 1.5)
line([iso(SX, GMAIL_Y, -eu, -ev), iso(SX, GMAIL_Y, 0, ev * 0.15), iso(SX, GMAIL_Y, eu, -ev)], INK, 1.5)

dashed(SX, CACHE_Y + 40, GMAIL_Y)

# 02 Local cache: a plate etched with inbox rows, one marked unread.
cache_corner = slab(CACHE_Y, A, PT, FACE_TOP, FACE_L, FACE_R)
for k in range(5):
    v = -A / 2 + A * (k + 1) / 6
    line([iso(SX, CACHE_Y, -A / 2 + 38, v), iso(SX, CACHE_Y, A / 2 - 20, v)], INK if k == 3 else HAIR, 1.5)
    if k == 3:
        px, py = iso(SX, CACHE_Y, -A / 2 + 22, v)
        d.ellipse((P(px - 4), P(py - 4), P(px + 4), P(py + 4)), fill=SIGNAL)

dashed(SX, KEY_BASE_Y + 40, CACHE_Y)

# 01 The key: a keycap with E on it, the only lit thing in the frame.
B, TB, KH = 118, 84, 58
kb = [iso(SX, KEY_BASE_Y, u, v) for u, v in [(-B / 2, -B / 2), (B / 2, -B / 2), (B / 2, B / 2), (-B / 2, B / 2)]]
kt = [iso(SX, KEY_BASE_Y, u, v, KH) for u, v in [(-TB / 2, -TB / 2), (TB / 2, -TB / 2), (TB / 2, TB / 2), (-TB / 2, TB / 2)]]
poly([kb[3], kb[2], kt[2], kt[3]], SIGNAL_MID, None)
poly([kb[2], kb[1], kt[1], kt[2]], SIGNAL_LOW, None)
poly(kt, SIGNAL, None)
for a, b in [(kb[3], kb[2]), (kb[2], kb[1]), (kb[3], kt[3]), (kb[2], kt[2]), (kb[1], kt[1])]:
    line([a, b], (255, 140, 96), 1.2)

# The legend, mapped onto the keycap's top face.
L = 200
legend = Image.new("L", (L, L), 0)
ImageDraw.Draw(legend).text((L / 2, L / 2), "E", font=ImageFont.truetype(MONO_BOLD, 150), fill=255, anchor="mm")
k = TB / L * SS * 0.62  # legend scale on the face
X0, Y0 = iso(SX, KEY_BASE_Y, 0, 0, KH)
X0, Y0 = X0 * SS, Y0 * SS
# Output pixel -> legend pixel (Image.transform maps backwards), so the
# letter lies flat on the cap and still reads upright.
a_ = 1 / (2 * COS30 * k)
b_ = 1 / k
coeffs = (a_, b_, -a_ * X0 - b_ * Y0 + L / 2, -a_, b_, a_ * X0 - b_ * Y0 + L / 2)
mask = legend.transform(img.size, Image.AFFINE, coeffs, resample=Image.BICUBIC)
img.paste(Image.new("RGB", img.size, BG), mask=mask)
key_corner = kt[1]

# --- Callouts: one per layer, read top to bottom -----------------------------

LABEL_X = 1900
for (ax, ay), num, name, note in [
    (key_corner, "01", "KEY", "E marks it done"),
    (cache_corner, "02", "LOCAL CACHE", "The window changes at once"),
    (gmail_corner, "03", "GMAIL", "Synced in the background"),
]:
    d.ellipse((P(ax - 4), P(ay - 4), P(ax + 4), P(ay + 4)), fill=INK)
    line([(ax + 10, ay), (LABEL_X - 24, ay)], HAIR, 1.5)
    text(LABEL_X, ay - 8, num, 17, SIGNAL, bold=True)
    text(LABEL_X + 52, ay - 8, name, 17, INK, bold=True)
    text(LABEL_X + 52, ay + 22, note.upper(), 15)

img.resize((W, H), Image.LANCZOS).save(OUT, optimize=True)
print(OUT)
