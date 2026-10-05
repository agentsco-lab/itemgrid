#!/usr/bin/env python3
"""The Surface Duo 1, open flat, seen from the front - two SVG layers for
item/grid's window: duo-body.svg under the live screens, duo-over.svg over them
(the inner bezels, the spine, the hinges). In millimetres, from agentsco.uk's
DuoBody (measured off a straight-on photo and Microsoft's spec sheet:
186.9 x 145.2 mm open). Run it to make the files beside it: the body whole and the over layer, and
for folding, each half (duo-left, duo-right) and the spine with its hinges."""
import os

BODY_W, BODY_H = 186.9, 145.2
OUTPUT_W, HINGE_W = 1392, 42
GAP = 3.7
HALF_W = (BODY_W - GAP) / 2
R, RIM, SIDE = 10, 0.5, 4.1
SCREEN_W = BODY_W - 2 * SIDE
PANEL_W = SCREEN_W * ((OUTPUT_W - HINGE_W) / 2) / OUTPUT_W
SCREEN_H = PANEL_W * (1800 / 1350)
SCREEN_TOP = (BODY_H - SCREEN_H) / 2
COL_X = SIDE + PANEL_W
COL_W = SCREEN_W - 2 * PANEL_W
NOTCH_W, NOTCH_D, NOTCH_FILLET = 3.65, 8.0, 0.6
LEDGE, SLIT = 1.2, 0.4
CHASSIS = "#c9ccc4"
BLOCK_W, BLOCK_TOP, BLOCK_H = 10.2, 1.7, 6.3
MID = BODY_W / 2


def half_path():
    w, h, nw, nd, f = HALF_W, BODY_H, NOTCH_W, NOTCH_D, NOTCH_FILLET
    x = w - nw
    return " ".join([
        f"M {R} 0", f"L {x - f} 0", f"Q {x} 0 {x} {f}", f"L {x} {nd - f}", f"Q {x} {nd} {x + f} {nd}", f"L {w} {nd}",
        f"L {w} {h - nd}", f"L {x + f} {h - nd}", f"Q {x} {h - nd} {x} {h - nd + f}", f"L {x} {h - f}", f"Q {x} {h} {x - f} {h}",
        f"L {R} {h}", f"A {R} {R} 0 0 1 0 {h - R}", f"L 0 {R}", f"A {R} {R} 0 0 1 {R} 0", "Z",
    ])


HALF = half_path()
DEFS = """<defs>
<linearGradient id="glass" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#111113"/><stop offset=".5" stop-color="#0a0a0b"/><stop offset="1" stop-color="#060607"/></linearGradient>
<linearGradient id="sheen" x1="0" y1="0" x2="1" y2="0.6"><stop offset="0" stop-color="#fff" stop-opacity=".05"/><stop offset=".35" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="barrel" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#1c1b19"/><stop offset=".12" stop-color="#5b5850"/><stop offset=".2" stop-color="#d9d6cc"/><stop offset=".27" stop-color="#6d6a61"/><stop offset=".45" stop-color="#262522"/><stop offset=".58" stop-color="#3a3833"/><stop offset=".75" stop-color="#8f8b80"/><stop offset=".86" stop-color="#4a4842"/><stop offset="1" stop-color="#171614"/></linearGradient>
<linearGradient id="barrel-shade" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#000" stop-opacity=".35"/><stop offset=".18" stop-color="#000" stop-opacity="0"/><stop offset=".82" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".35"/></linearGradient>
<linearGradient id="rod" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#0d0d0b"/><stop offset=".3" stop-color="#3f3f35"/><stop offset=".42" stop-color="#b8b7a2"/><stop offset=".52" stop-color="#55554a"/><stop offset="1" stop-color="#101010"/></linearGradient>
<radialGradient id="sensor" cx=".4" cy=".35" r=".7"><stop offset="0" stop-color="#d8d2b4"/><stop offset=".55" stop-color="#9d9780"/><stop offset="1" stop-color="#5a5749"/></radialGradient>
<radialGradient id="lens" cx=".4" cy=".35" r=".6"><stop offset="0" stop-color="#3d4a60"/><stop offset=".5" stop-color="#0b0d12"/><stop offset="1" stop-color="#000"/></radialGradient>
<clipPath id="hclip"><path d="%s"/></clipPath>
</defs>""" % HALF


def half(mirror):
    t = f' transform="translate({BODY_W} 0) scale(-1 1)"' if mirror else ""
    return (f'<g{t}><path d="{HALF}" fill="url(#glass)"/>'
            f'<path d="{HALF}" fill="none" stroke="{CHASSIS}" stroke-width="{RIM * 2}" clip-path="url(#hclip)"/>'
            f'<path d="{HALF}" fill="url(#sheen)" clip-path="url(#hclip)"/></g>')


def block(bottom):
    t = f' transform="translate(0 {BODY_H}) scale(1 -1)"' if bottom else ""
    x = MID - BLOCK_W / 2
    return (f'<g{t}><rect x="{x}" y="{BLOCK_TOP}" width="{BLOCK_W}" height="{BLOCK_H}" rx="0.5" fill="url(#barrel)" stroke="#050505" stroke-width="0.1"/>'
            f'<rect x="{x}" y="{BLOCK_TOP}" width="{BLOCK_W}" height="{BLOCK_H}" rx="0.5" fill="url(#barrel-shade)"/>'
            f'<rect x="{MID - GAP / 2}" y="{BLOCK_TOP + BLOCK_H - 0.15}" width="{GAP}" height="0.3" fill="#050505"/></g>')


def spine():
    x0, y0 = MID - GAP / 2, BLOCK_TOP + BLOCK_H
    h = BODY_H - 2 * y0
    out = f'<rect x="{x0}" y="{y0}" width="{GAP}" height="{h}" fill="#040405"/>'
    for dx, w in [(0.7, 0.75), (2.25, 0.75)]:
        out += f'<rect x="{x0 + dx}" y="{y0}" width="{w}" height="{h}" fill="url(#rod)"/>'
    return out


def svg(inner):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {BODY_W} {BODY_H}" width="{BODY_W * 4}" height="{BODY_H * 4}">{DEFS}{inner}</svg>\n'


body = ""
for y in (LEDGE, BODY_H - NOTCH_D):
    body += f'<rect x="{HALF_W - NOTCH_W}" y="{y}" width="{BODY_W - 2 * (HALF_W - NOTCH_W)}" height="{NOTCH_D - LEDGE}" fill="#060607"/>'
body += half(False) + half(True)
for y in (0, BODY_H - LEDGE):
    x = HALF_W - NOTCH_W - 0.3
    w = MID - SLIT / 2 - x
    body += f'<rect x="{x}" y="{y}" width="{w}" height="{LEDGE}" rx="0.15" fill="{CHASSIS}"/>'
    body += f'<rect x="{MID + SLIT / 2}" y="{y}" width="{w}" height="{LEDGE}" rx="0.15" fill="{CHASSIS}"/>'
    body += f'<rect x="{HALF_W - NOTCH_W}" y="{LEDGE if y == 0 else y - 0.15}" width="{BODY_W - 2 * (HALF_W - NOTCH_W)}" height="0.15" fill="#000" opacity="0.5"/>'
body += f'<rect x="{BODY_W - 47.0 - 5.55}" y="{8.3 - 0.75}" width="11.1" height="1.5" rx="0.75" fill="#26272a" stroke="#3a3b3e" stroke-width="0.1"/>'
body += f'<circle cx="{BODY_W - 34.3}" cy="8.5" r="1.75" fill="#111216" stroke="#2c2e33" stroke-width="0.18"/>'
body += f'<circle cx="{BODY_W - 34.3}" cy="8.5" r="0.8" fill="url(#lens)"/>'
body += f'<circle cx="{BODY_W - 17.6}" cy="8.3" r="1.6" fill="url(#sensor)" stroke="#4a4840" stroke-width="0.12"/>'
# The screens' place, black until a picture comes.
body += f'<rect x="{SIDE}" y="{SCREEN_TOP}" width="{SCREEN_W}" height="{SCREEN_H}" fill="#000"/>'

over = f'<rect x="{COL_X}" y="{SCREEN_TOP}" width="{COL_W}" height="{SCREEN_H}" fill="#0c0c0d"/>' + spine() + block(False) + block(True)

# Each half on its own (item/grid folds the right one about the spine): the
# body and the dead column cut at the middle.
body_halves = body + f'<rect x="{COL_X}" y="{SCREEN_TOP}" width="{COL_W}" height="{SCREEN_H}" fill="#0c0c0d"/>'
def cut(inner, left):
    x, w = (0, MID) if left else (MID, BODY_W - MID)
    return f'<clipPath id="cut"><rect x="{x}" y="0" width="{w}" height="{BODY_H}"/></clipPath><g clip-path="url(#cut)">{inner}</g>'

here = os.path.dirname(os.path.abspath(__file__))
open(os.path.join(here, "duo-body.svg"), "w").write(svg(body))
open(os.path.join(here, "duo-over.svg"), "w").write(svg(over))
open(os.path.join(here, "duo-left.svg"), "w").write(svg(cut(body_halves, True)))
open(os.path.join(here, "duo-right.svg"), "w").write(svg(cut(body_halves, False)))
# The hinge as a strip the barrels' width (item/grid turns it to face the
# viewer, a cylinder): the barrels and the rods between, at the strip's middle.
def strip(inner):
    x0 = MID - BLOCK_W / 2
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0} 0 {BLOCK_W} {BODY_H}" width="{BLOCK_W * 4}" height="{BODY_H * 4}">{DEFS}{inner}</svg>\n'
open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "duo-hinge.svg"), "w").write(strip(spine() + block(False) + block(True)))
# The spine and the hinges alone, over both halves.
open(os.path.join(here, "duo-spine.svg"), "w").write(svg(spine() + block(False) + block(True)))
print(f"panel {PANEL_W:.3f} x {SCREEN_H:.3f} mm at ({SIDE}, {SCREEN_TOP:.3f}); right at {COL_X + COL_W:.3f}")
