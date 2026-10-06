"""Extract per-provider SVGs from pi-web's provider-icons.svg sprite.

Reads D:/github/---ai-tools---/pi-web/public/provider-icons.svg, writes each
<symbol id="x" viewBox="...">…</symbol> as a standalone
crates/app/assets/icons/provider/x.svg (symbol -> svg root, xmlns added).
The files keep their original path fill attributes; gpui only uses the alpha
channel and tints with the caller's color, so shape is all that matters.
"""
import re
import pathlib

SRC = pathlib.Path(r"D:/github/---ai-tools---/pi-web/public/provider-icons.svg")
OUT = pathlib.Path(r"D:/ai_workspace/pi-flash/crates/app/assets/icons/provider")

text = SRC.read_text(encoding="utf-8")
symbols = re.findall(r'<symbol id="([^"]+)" (viewBox="[^"]+")>(.*?)</symbol>', text, re.S)
assert symbols, "no symbols found in sprite"

OUT.mkdir(parents=True, exist_ok=True)
written = []
for sid, vb, body in symbols:
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" %s>\n'
        "  <!-- extracted from pi-web public/provider-icons.svg; icons derived\n"
        "       from @lobehub/icons (MIT, (c) 2023 LobeHub) -->\n"
        "  %s\n</svg>\n" % (vb, body.strip())
    )
    (OUT / f"{sid}.svg").write_text(svg, encoding="utf-8", newline="\n")
    written.append(sid)

print(len(written), "written:", " ".join(written))
