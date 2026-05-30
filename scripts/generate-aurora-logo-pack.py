#!/usr/bin/env python3
from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


OUT = Path("output/aurora-server-brand-pack")

COLORS = {
    "page": "#07131c",
    "nav": "#07111a",
    "panel": "#102330",
    "panel_strong": "#13293a",
    "border": "#24536c",
    "cyan": "#29b6f6",
    "cyan_strong": "#67cbfa",
    "cyan_deep": "#1c7fac",
    "rose": "#f9a8c4",
    "rose_deep": "#c46b88",
    "violet": "#a78bfa",
    "violet_deep": "#7c3aed",
    "success": "#7dd3c7",
    "warn": "#c6a36b",
    "text": "#e6f4fb",
    "muted": "#a7bcc9",
}

SERVERS = [
    ("rustifi", "wifi", "Wi-Fi signal over stacked planes", "cyan"),
    ("rustify", "play", "media/play wave over stacked planes", "rose"),
    ("unrust", "grid", "storage grid over stacked planes", "warn"),
    ("rustscale", "scale", "scale arrows over stacked planes", "success"),
    ("synapse2", "synapse", "node synapse over stacked planes", "violet"),
    ("rustarr", "radar", "radar/arr sweep over stacked planes", "cyan_strong"),
    ("cortex", "logs", "log stream over stacked planes", "success"),
    ("rustcane", "rune", "command rune over stacked planes", "violet"),
    ("rmcp-template", "brackets", "MCP brackets over stacked planes", "cyan"),
    ("apprise-mcp", "bell", "notification bell over stacked planes", "rose"),
    ("axon", "graph", "axon graph over stacked planes", "violet"),
    ("labby", "stack", "canonical stacked-plane mark", "cyan"),
]


def hex_to_rgb(value: str) -> tuple[int, int, int]:
    value = value.lstrip("#")
    return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4))


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    paths = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf" if bold else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf" if bold else "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    ]
    for path in paths:
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            pass
    return ImageFont.load_default()


def svc_dir(name: str) -> Path:
    path = OUT / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def accent(name: str) -> str:
    return COLORS[name]


def svg_layers() -> str:
    return f"""
  <path d="M24 28 L64 13 L104 28 L64 43 Z" fill="{COLORS['border']}" opacity=".96"/>
  <path d="M24 48 L64 33 L104 48 L64 63 Z" fill="{COLORS['cyan_deep']}" opacity=".92"/>
  <path d="M24 68 L64 53 L104 68 L64 83 Z" fill="{COLORS['cyan']}" opacity=".88"/>
  <path d="M24 88 L64 73 L104 88 L64 103 Z" fill="{COLORS['cyan_strong']}" opacity=".90"/>
"""


def svg_motif(kind: str, color: str) -> str:
    c = accent(color)
    common = f'fill="none" stroke="{c}" stroke-width="5.5" stroke-linecap="round" stroke-linejoin="round"'
    if kind == "wifi":
        return f'<path d="M42 48 Q64 30 86 48" {common}/><path d="M51 59 Q64 49 77 59" {common}/><circle cx="64" cy="70" r="4.5" fill="{c}"/>'
    if kind == "play":
        return f'<path d="M48 40 L48 76 L81 58 Z" fill="{c}"/><path d="M39 84 Q64 97 89 84" {common} opacity=".72"/>'
    if kind == "grid":
        cells = "".join(f'<rect x="{44+x*16}" y="{38+y*16}" width="10" height="10" rx="2" fill="{c}" opacity="{.65 + .1*(x+y)}"/>' for y in range(3) for x in range(3))
        return cells
    if kind == "scale":
        return f'<path d="M44 61 H84 M64 41 V81 M45 42 L64 61 L83 42 M45 80 L64 61 L83 80" {common}/>'
    if kind == "synapse":
        return f'<path d="M45 66 L61 49 L82 59 M61 49 L76 36 M61 49 L70 76" {common}/><circle cx="45" cy="66" r="5" fill="{c}"/><circle cx="61" cy="49" r="6" fill="{c}"/><circle cx="82" cy="59" r="5" fill="{c}"/><circle cx="76" cy="36" r="4.5" fill="{c}"/><circle cx="70" cy="76" r="4.5" fill="{c}"/>'
    if kind == "radar":
        return f'<path d="M42 74 A25 25 0 1 1 86 74" {common}/><path d="M64 74 L84 47" {common}/><circle cx="64" cy="74" r="4.5" fill="{c}"/>'
    if kind == "logs":
        return f'<path d="M42 43 H86 M42 58 H77 M42 73 H86" {common}/><path d="M35 43 H35.5 M35 58 H35.5 M35 73 H35.5" {common}/>'
    if kind == "rune":
        return f'<path d="M64 34 L45 57 L64 82 L83 57 Z M50 57 H78 M64 34 V82" {common}/>'
    if kind == "brackets":
        return f'<path d="M51 40 H39 V78 H51 M77 40 H89 V78 H77 M57 72 L72 46" {common}/>'
    if kind == "bell":
        return f'<path d="M47 72 H81 L76 63 V53 C76 44 71 38 64 38 C57 38 52 44 52 53 V63 Z M59 81 H69" {common}/>'
    if kind == "graph":
        return f'<path d="M42 70 L61 45 L85 62 M61 45 L78 35 M61 45 L69 82" {common}/><circle cx="42" cy="70" r="5" fill="{c}"/><circle cx="61" cy="45" r="6" fill="{c}"/><circle cx="85" cy="62" r="5" fill="{c}"/><circle cx="78" cy="35" r="4.5" fill="{c}"/><circle cx="69" cy="82" r="4.5" fill="{c}"/>'
    return ""


def mark_svg(name: str, kind: str, color: str) -> str:
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 128 128" role="img" aria-label="{name} mark">
  <defs>
    <filter id="glow" x="-40%" y="-40%" width="180%" height="180%">
      <feGaussianBlur stdDeviation="2.5" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <rect width="128" height="128" rx="28" fill="{COLORS['page']}"/>
  <rect x="9" y="9" width="110" height="110" rx="24" fill="{COLORS['panel']}" stroke="{COLORS['border']}" stroke-width="2"/>
  <g filter="url(#glow)">
{svg_layers()}
  </g>
  <g>{svg_motif(kind, color)}</g>
</svg>
"""


def lockup_svg(name: str, kind: str, color: str) -> str:
    safe = name.replace("&", "&amp;")
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="960" height="256" viewBox="0 0 960 256" role="img" aria-label="{safe} logo">
  <rect width="960" height="256" rx="36" fill="{COLORS['page']}"/>
  <rect x="24" y="24" width="912" height="208" rx="30" fill="{COLORS['panel']}" stroke="{COLORS['border']}" stroke-width="2"/>
  <svg x="52" y="48" width="160" height="160" viewBox="0 0 128 128">
{svg_layers()}
    <g>{svg_motif(kind, color)}</g>
  </svg>
  <text x="244" y="133" fill="{COLORS['text']}" font-family="Manrope, Inter, system-ui, sans-serif" font-size="64" font-weight="800" letter-spacing="0">{safe}</text>
  <text x="248" y="172" fill="{COLORS['muted']}" font-family="Inter, system-ui, sans-serif" font-size="24" font-weight="560">Aurora server identity</text>
  <circle cx="882" cy="128" r="13" fill="{accent(color)}"/>
</svg>
"""


def draw_layers(draw: ImageDraw.ImageDraw, scale: float = 1.0, ox: float = 0, oy: float = 0) -> None:
    layers = [
        (28, COLORS["border"], 0.96),
        (48, COLORS["cyan_deep"], 0.92),
        (68, COLORS["cyan"], 0.88),
        (88, COLORS["cyan_strong"], 0.90),
    ]
    for y, color, opacity in layers:
        pts = [(24, y), (64, y - 15), (104, y), (64, y + 15)]
        pts = [(ox + x * scale, oy + yy * scale) for x, yy in pts]
        rgba = (*hex_to_rgb(color), round(255 * opacity))
        draw.polygon(pts, fill=rgba)


def draw_line(draw: ImageDraw.ImageDraw, pts, color, width=5, scale=1.0, ox=0, oy=0):
    pts = [(ox + x * scale, oy + y * scale) for x, y in pts]
    draw.line(pts, fill=color, width=max(1, round(width * scale)), joint="curve")


def draw_motif(draw: ImageDraw.ImageDraw, kind: str, color: str, scale: float = 1.0, ox: float = 0, oy: float = 0) -> None:
    c = (*hex_to_rgb(accent(color)), 255)
    w = max(2, round(5 * scale))
    def xy(v): return [ox + v[0] * scale, oy + v[1] * scale, ox + v[2] * scale, oy + v[3] * scale]
    if kind == "wifi":
        draw.arc(xy((38, 29, 90, 69)), 205, 335, fill=c, width=w)
        draw.arc(xy((49, 47, 79, 75)), 210, 330, fill=c, width=w)
        draw.ellipse(xy((59, 66, 69, 76)), fill=c)
    elif kind == "play":
        draw.polygon([(ox + 48*scale, oy + 40*scale), (ox + 48*scale, oy + 76*scale), (ox + 82*scale, oy + 58*scale)], fill=c)
        draw.arc(xy((35, 66, 93, 99)), 28, 152, fill=c, width=w)
    elif kind == "grid":
        for y in range(3):
            for x in range(3):
                draw.rounded_rectangle(xy((44+x*16, 38+y*16, 54+x*16, 48+y*16)), radius=round(2*scale), fill=c)
    elif kind == "scale":
        draw_line(draw, [(44,61),(84,61)], c, 5, scale, ox, oy); draw_line(draw, [(64,41),(64,81)], c, 5, scale, ox, oy)
        draw_line(draw, [(45,42),(64,61),(83,42)], c, 5, scale, ox, oy); draw_line(draw, [(45,80),(64,61),(83,80)], c, 5, scale, ox, oy)
    elif kind in {"synapse", "graph"}:
        pts = [(45,66),(61,49),(82,59),(61,49),(76,36),(61,49),(70,76)]
        draw_line(draw, pts[:3], c, 5, scale, ox, oy); draw_line(draw, pts[3:5], c, 5, scale, ox, oy); draw_line(draw, pts[5:], c, 5, scale, ox, oy)
        for cx, cy, r in [(45,66,5),(61,49,6),(82,59,5),(76,36,5),(70,76,5)]:
            draw.ellipse(xy((cx-r, cy-r, cx+r, cy+r)), fill=c)
    elif kind == "radar":
        draw.arc(xy((39, 48, 89, 98)), 185, 355, fill=c, width=w)
        draw_line(draw, [(64,74),(84,47)], c, 5, scale, ox, oy); draw.ellipse(xy((59,69,69,79)), fill=c)
    elif kind == "logs":
        for y, x2 in [(43,86),(58,77),(73,86)]:
            draw_line(draw, [(42,y),(x2,y)], c, 5, scale, ox, oy); draw.ellipse(xy((33,y-2,38,y+3)), fill=c)
    elif kind == "rune":
        draw_line(draw, [(64,34),(45,57),(64,82),(83,57),(64,34)], c, 5, scale, ox, oy); draw_line(draw, [(50,57),(78,57)], c, 5, scale, ox, oy); draw_line(draw, [(64,34),(64,82)], c, 5, scale, ox, oy)
    elif kind == "brackets":
        draw_line(draw, [(51,40),(39,40),(39,78),(51,78)], c, 5, scale, ox, oy); draw_line(draw, [(77,40),(89,40),(89,78),(77,78)], c, 5, scale, ox, oy); draw_line(draw, [(57,72),(72,46)], c, 5, scale, ox, oy)
    elif kind == "bell":
        draw_line(draw, [(47,72),(81,72),(76,63),(76,53),(73,44),(64,38),(55,44),(52,53),(52,63),(47,72)], c, 5, scale, ox, oy); draw_line(draw, [(59,81),(69,81)], c, 5, scale, ox, oy)


def render_mark_png(name: str, kind: str, color: str, size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (*hex_to_rgb(COLORS["page"]), 255))
    draw = ImageDraw.Draw(img, "RGBA")
    pad = round(size * 0.07)
    draw.rounded_rectangle((pad, pad, size - pad, size - pad), radius=round(size * 0.19), fill=(*hex_to_rgb(COLORS["panel"]), 255), outline=(*hex_to_rgb(COLORS["border"]), 255), width=max(1, size // 128))
    scale = size / 128
    draw_layers(draw, scale)
    draw_motif(draw, kind, color, scale)
    return img


def render_lockup_png(name: str, kind: str, color: str) -> Image.Image:
    img = Image.new("RGBA", (960, 256), (*hex_to_rgb(COLORS["page"]), 255))
    draw = ImageDraw.Draw(img, "RGBA")
    draw.rounded_rectangle((24, 24, 936, 232), radius=30, fill=(*hex_to_rgb(COLORS["panel"]), 255), outline=(*hex_to_rgb(COLORS["border"]), 255), width=2)
    icon = render_mark_png(name, kind, color, 160)
    img.alpha_composite(icon, (52, 48))
    draw.text((244, 68), name, font=font(64, True), fill=COLORS["text"])
    draw.text((248, 148), "Aurora server identity", font=font(24), fill=COLORS["muted"])
    draw.ellipse((869, 115, 895, 141), fill=COLORS[color])
    return img


def render_contact_sheet() -> Image.Image:
    tile = 220
    cols = 4
    rows = 3
    img = Image.new("RGBA", (cols * tile, rows * tile), (*hex_to_rgb(COLORS["page"]), 255))
    draw = ImageDraw.Draw(img, "RGBA")
    label_font = font(20, True)
    meta_font = font(13)
    for idx, (name, kind, _description, color) in enumerate(SERVERS):
        col = idx % cols
        row = idx // cols
        x = col * tile
        y = row * tile
        draw.rounded_rectangle((x + 14, y + 14, x + tile - 14, y + tile - 14), radius=22, fill=(*hex_to_rgb(COLORS["panel"]), 255), outline=(*hex_to_rgb(COLORS["border"]), 255), width=1)
        icon = render_mark_png(name, kind, color, 112)
        img.alpha_composite(icon, (x + 54, y + 34))
        draw.text((x + 24, y + 160), name, font=label_font, fill=COLORS["text"])
        draw.text((x + 24, y + 188), color.replace("_", " "), font=meta_font, fill=COLORS["muted"])
    return img


def write_pack() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    manifest = {"source": "Aurora design system inspired server identity pack", "colors": COLORS, "servers": []}
    for name, kind, description, color in SERVERS:
        path = svc_dir(name)
        (path / "mark.svg").write_text(mark_svg(name, kind, color), encoding="utf-8")
        (path / "favicon.svg").write_text(mark_svg(name, kind, color), encoding="utf-8")
        (path / "logo-lockup.svg").write_text(lockup_svg(name, kind, color), encoding="utf-8")
        for size in (16, 32, 48, 180, 192, 512, 1024):
            render_mark_png(name, kind, color, size).save(path / f"icon-{size}.png")
        render_lockup_png(name, kind, color).save(path / "logo-lockup.png")
        ico_frames = [render_mark_png(name, kind, color, s) for s in (16, 32, 48)]
        ico_frames[0].save(path / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)], append_images=ico_frames[1:])
        manifest["servers"].append({
            "name": name,
            "motif": description,
            "accent": color,
            "files": ["mark.svg", "favicon.svg", "favicon.ico", "logo-lockup.svg", "logo-lockup.png", "icon-16.png", "icon-32.png", "icon-48.png", "icon-180.png", "icon-192.png", "icon-512.png", "icon-1024.png"],
        })
    render_contact_sheet().save(OUT / "contact-sheet.png")
    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (OUT / "README.md").write_text(readme(), encoding="utf-8")


def readme() -> str:
    names = "\n".join(f"- `{name}/` — {desc}" for name, _, desc, _ in SERVERS)
    return f"""# Aurora Server Brand Pack

Generated from the Aurora design-system direction in `../aurora-design-system`:
dark navy surfaces, cyan primary, rose secondary, violet automation accent, muted status colors, and the Labby stacked-plane mark language.

Each server folder contains:

- `mark.svg` — square SVG master
- `logo-lockup.svg` and `logo-lockup.png` — horizontal lockup
- `favicon.svg` and `favicon.ico` — browser favicon assets
- `icon-16.png`, `icon-32.png`, `icon-48.png`, `icon-180.png`, `icon-192.png`, `icon-512.png`, `icon-1024.png` — common app/icon sizes

Top-level previews:

- `contact-sheet.png` — generated preview of the deterministic pack
- `concept-sheet-generated.png` — original AI-generated concept pass, copied from Codex image generation output

## Servers

{names}

## Notes

The bitmap assets are rendered from the same geometric source as the SVGs. The image-generation pass was used as a concept direction; production files are deterministic so favicons remain crisp and repeatable.
"""


if __name__ == "__main__":
    write_pack()
    print(f"Wrote {OUT}")
