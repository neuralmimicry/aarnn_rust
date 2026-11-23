"""Minimal writer for editable LibreOffice Draw (.odg) posters.

Every text block is a native, reflowing text frame or shape text; figures are
embedded SVG (with PNG fallback). Text heights are predicted with the real font
metrics (Pillow) so that blocks can be stacked without overlap.
"""
from __future__ import annotations

import io
import zipfile
from dataclasses import dataclass, field
from xml.sax.saxutils import escape

from PIL import ImageFont

PT_TO_CM = 2.54 / 72.0
FONT_FAMILY = "Liberation Sans"
FONT_FILES = {
    (False, False): "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    (True, False): "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    (False, True): "/usr/share/fonts/truetype/liberation/LiberationSans-Italic.ttf",
    (True, True): "/usr/share/fonts/truetype/liberation/LiberationSans-BoldItalic.ttf",
}
_MEASURE_PX = 200
_font_cache: dict = {}


def _font(bold: bool, italic: bool) -> ImageFont.FreeTypeFont:
    key = (bold, italic)
    if key not in _font_cache:
        _font_cache[key] = ImageFont.truetype(FONT_FILES[key], _MEASURE_PX)
    return _font_cache[key]


def text_width_cm(text: str, size_pt: float, bold=False, italic=False) -> float:
    return _font(bold, italic).getlength(text) / _MEASURE_PX * size_pt * PT_TO_CM


@dataclass
class Run:
    text: str
    bold: bool | None = None
    italic: bool = False
    color: str | None = None
    size: float | None = None


@dataclass
class PStyle:
    size: float = 24.0
    leading: float = 1.28
    color: str = "#1D2B36"
    bold: bool = False
    italic: bool = False
    align: str = "start"  # start | center | end
    before: float = 0.0  # cm
    after: float = 0.0  # cm
    hang: float = 0.0  # cm hanging indent (bullets)
    letter_spacing: float = 0.0  # pt

    def line_cm(self) -> float:
        return self.size * self.leading * PT_TO_CM


@dataclass
class Para:
    runs: list
    style: PStyle
    bullet: str | None = None


def runs_from_markup(text: str) -> list:
    """Tiny markup: **bold** segments; everything else regular."""
    out = []
    parts = text.split("**")
    for i, part in enumerate(parts):
        if part:
            out.append(Run(part, bold=True if i % 2 == 1 else None))
    return out


def P(text: str | list, style: PStyle, bullet: str | None = None) -> Para:
    runs = runs_from_markup(text) if isinstance(text, str) else text
    return Para(runs, style, bullet)


def _tokens(para: Para):
    """Yield (word, trailing_space, bold, italic, size) tokens; '\n' forces a break."""
    st = para.style
    for run in para.runs:
        bold = st.bold if run.bold is None else run.bold
        italic = st.italic or run.italic
        size = run.size or st.size
        chunks = run.text.split("\n")
        for ci, chunk in enumerate(chunks):
            if ci > 0:
                yield ("\n", "", bold, italic, size)
            words = chunk.split(" ")
            for wi, w in enumerate(words):
                space = " " if wi < len(words) - 1 else ""
                if w or space:
                    yield (w, space, bold, italic, size)


def count_lines(para: Para, width_cm: float, safety: float = 0.995) -> int:
    st = para.style
    width = width_cm * safety
    first_w = width
    rest_w = width - st.hang
    if para.bullet:
        pass  # bullet occupies the hanging indent on the first line
    lines = 1
    cur = st.hang if para.bullet else 0.0
    avail = first_w
    pending_space = 0.0
    for word, space, bold, italic, size in _tokens(para):
        if word == "\n":
            lines += 1
            cur = 0.0
            avail = rest_w
            pending_space = 0.0
            continue
        ww = text_width_cm(word, size, bold, italic)
        sw = text_width_cm(space, size, bold, italic) if space else 0.0
        if cur > 0 and cur + pending_space + ww > avail:
            lines += 1
            cur = ww
            avail = rest_w
        else:
            cur += pending_space + ww
        pending_space = sw
    return lines


def para_height(para: Para, width_cm: float) -> float:
    st = para.style
    return st.before + count_lines(para, width_cm) * st.line_cm() + st.after


def block_height(paras: list, width_cm: float) -> float:
    return sum(para_height(p, width_cm) for p in paras)


@dataclass
class Doc:
    width: float
    height: float
    title: str = ""
    creator: str = ""
    description: str = ""
    shapes: list = field(default_factory=list)
    gstyles: dict = field(default_factory=dict)
    pstyles: dict = field(default_factory=dict)
    tstyles: dict = field(default_factory=dict)
    pictures: dict = field(default_factory=dict)
    _n: int = 0

    # ------------------------------------------------------------------ styles
    def _g(self, **props) -> str:
        key = tuple(sorted(props.items()))
        if key not in self.gstyles:
            self.gstyles[key] = f"gr{len(self.gstyles) + 1}"
        return self.gstyles[key]

    def _p(self, st: PStyle, bullet: bool) -> str:
        key = (st.size, st.leading, st.color, st.bold, st.italic, st.align,
               st.before, st.after, st.hang if bullet else 0.0, st.letter_spacing)
        if key not in self.pstyles:
            self.pstyles[key] = f"P{len(self.pstyles) + 1}"
        return self.pstyles[key]

    def _t(self, bold, italic, color, size) -> str:
        key = (bold, italic, color, size)
        if key not in self.tstyles:
            self.tstyles[key] = f"T{len(self.tstyles) + 1}"
        return self.tstyles[key]

    def _name(self, prefix: str) -> str:
        self._n += 1
        return f"{prefix} {self._n}"

    # ------------------------------------------------------------------ text
    def _para_xml(self, para: Para) -> str:
        st = para.style
        pname = self._p(st, bool(para.bullet))
        out = [f'<text:p text:style-name="{pname}">']
        if para.bullet:
            out.append(f'<text:span text:style-name="{self._t(True, False, st.color if st.color else None, st.size)}">'
                       f'{escape(para.bullet)}</text:span>')
        for run in para.runs:
            bold = st.bold if run.bold is None else run.bold
            color = run.color or st.color
            size = run.size or st.size
            tname = self._t(bold, st.italic or run.italic, color, size)
            pieces = run.text.split("\n")
            body = []
            for i, piece in enumerate(pieces):
                if i:
                    body.append("<text:line-break/>")
                body.append(_spaces(escape(piece)))
            out.append(f'<text:span text:style-name="{tname}">{"".join(body)}</text:span>')
        out.append("</text:p>")
        return "".join(out)

    def text(self, x, y, w, paras, h=None, name=None, fill=None, pad=0.0, valign="top") -> float:
        """Native text frame. Returns its height (cm)."""
        inner_w = w - 2 * pad
        th = block_height(paras, inner_w) + 2 * pad
        h = max(h or 0.0, th)
        props = dict(stroke="none", fill="none" if not fill else "solid", fill_color=fill or "",
                     pad=pad, valign=valign, grow="true")
        g = self._g(kind="frame", **props)
        xml = "".join(self._para_xml(p) for p in paras)
        self.shapes.append(
            f'<draw:frame draw:style-name="{g}" draw:name="{escape(name or self._name("Text"))}" '
            f'svg:x="{x:.4f}cm" svg:y="{y:.4f}cm" svg:width="{w:.4f}cm" svg:height="{h:.4f}cm">'
            f'<draw:text-box>{xml}</draw:text-box></draw:frame>')
        return h

    def rect(self, x, y, w, h, fill=None, stroke=None, sw=0.05, radius=0.0, name=None,
             paras=None, pad=0.4, valign="middle", dash=False):
        g = self._g(kind="rect", fill="solid" if fill else "none", fill_color=fill or "",
                    stroke=("dash" if dash else "solid") if stroke else "none",
                    stroke_color=stroke or "", sw=sw if stroke else 0.0, pad=pad, valign=valign)
        nm = escape(name or self._name("Shape"))
        geo = f'svg:x="{x:.4f}cm" svg:y="{y:.4f}cm" svg:width="{w:.4f}cm" svg:height="{h:.4f}cm"'
        if not paras:
            rad = f' draw:corner-radius="{radius:.3f}cm"' if radius else ""
            self.shapes.append(f'<draw:rect draw:style-name="{g}" draw:name="{nm}" {geo}{rad}/>')
            return
        body = "".join(self._para_xml(p) for p in paras)
        self.shapes.append(
            f'<draw:custom-shape draw:style-name="{g}" draw:name="{nm}" {geo}>{body}'
            '<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" '
            'draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>')

    def ellipse(self, x, y, w, h, fill, stroke=None, sw=0.05, name=None):
        g = self._g(kind="rect", fill="solid" if fill else "none", fill_color=fill or "",
                    stroke="solid" if stroke else "none", stroke_color=stroke or "",
                    sw=sw if stroke else 0.0, pad=0.0, valign="middle")
        self.shapes.append(
            f'<draw:ellipse draw:style-name="{g}" draw:name="{escape(name or self._name("Ellipse"))}" '
            f'svg:x="{x:.4f}cm" svg:y="{y:.4f}cm" svg:width="{w:.4f}cm" svg:height="{h:.4f}cm"/>')

    def line(self, x1, y1, x2, y2, color="#7A8B96", sw=0.06, end=False, start=False,
             dash=False, name=None, head=0.55):
        g = self._g(kind="line", stroke="dash" if dash else "solid", stroke_color=color, sw=sw,
                    end="Arrow" if end else "", start="Arrow" if start else "", head=head)
        self.shapes.append(
            f'<draw:line draw:style-name="{g}" draw:name="{escape(name or self._name("Line"))}" '
            f'svg:x1="{x1:.4f}cm" svg:y1="{y1:.4f}cm" svg:x2="{x2:.4f}cm" svg:y2="{y2:.4f}cm"/>')

    def polyline(self, pts, color="#7A8B96", sw=0.06, end=False, start=False, name=None, head=0.55):
        g = self._g(kind="line", stroke="solid", stroke_color=color, sw=sw,
                    end="Arrow" if end else "", start="Arrow" if start else "", head=head)
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        x0, y0 = min(xs), min(ys)
        w = max(max(xs) - x0, 0.001)
        h = max(max(ys) - y0, 0.001)
        scale = 1000.0
        vb = f"0 0 {int(round(w * scale))} {int(round(h * scale))}"
        points = " ".join(f"{int(round((px - x0) * scale))},{int(round((py - y0) * scale))}" for px, py in pts)
        self.shapes.append(
            f'<draw:polyline draw:style-name="{g}" draw:name="{escape(name or self._name("Connector"))}" '
            f'svg:x="{x0:.4f}cm" svg:y="{y0:.4f}cm" svg:width="{w:.4f}cm" svg:height="{h:.4f}cm" '
            f'svg:viewBox="{vb}" draw:points="{points}"/>')

    def image(self, x, y, w, h, name, svg: bytes, png: bytes | None = None):
        svg_path = f"Pictures/{name}.svg"
        self.pictures[svg_path] = ("image/svg+xml", svg)
        alt = ""
        if png is not None:
            png_path = f"Pictures/{name}.png"
            self.pictures[png_path] = ("image/png", png)
            alt = (f'<draw:image xlink:href="{png_path}" xlink:type="simple" xlink:show="embed" '
                   f'xlink:actuate="onLoad" draw:mime-type="image/png"/>')
        self.shapes.append(
            f'<draw:frame draw:name="{escape(name)}" svg:x="{x:.4f}cm" svg:y="{y:.4f}cm" '
            f'svg:width="{w:.4f}cm" svg:height="{h:.4f}cm">'
            f'<draw:image xlink:href="{svg_path}" xlink:type="simple" xlink:show="embed" '
            f'xlink:actuate="onLoad" draw:mime-type="image/svg+xml"/>{alt}</draw:frame>')

    # ------------------------------------------------------------------ output
    def _gstyle_xml(self) -> str:
        out = []
        for key, name in self.gstyles.items():
            p = dict(key)
            kind = p["kind"]
            attrs = []
            if kind in ("frame", "rect"):
                attrs.append(f'draw:fill="{p["fill"]}"')
                if p["fill"] == "solid":
                    attrs.append(f'draw:fill-color="{p["fill_color"]}"')
                if p.get("stroke") == "dash":
                    attrs.append('draw:stroke="dash" draw:stroke-dash="Dash"')
                else:
                    attrs.append(f'draw:stroke="{p.get("stroke", "none")}"')
                if p.get("stroke") in ("solid", "dash"):
                    attrs.append(f'svg:stroke-color="{p["stroke_color"]}" svg:stroke-width="{p["sw"]:.3f}cm"')
                pad = p.get("pad", 0.0)
                attrs.append(f'fo:padding-left="{pad:.3f}cm" fo:padding-right="{pad:.3f}cm" '
                             f'fo:padding-top="{pad:.3f}cm" fo:padding-bottom="{pad:.3f}cm"')
                attrs.append(f'draw:textarea-vertical-align="{p.get("valign", "top")}"')
                attrs.append('draw:textarea-horizontal-align="justify" fo:wrap-option="wrap" '
                             'draw:auto-grow-width="false"')
                attrs.append(f'draw:auto-grow-height="{p.get("grow", "false")}"')
                attrs.append('draw:shadow="hidden"')
            elif kind == "line":
                if p["stroke"] == "dash":
                    attrs.append('draw:stroke="dash" draw:stroke-dash="Dash"')
                else:
                    attrs.append('draw:stroke="solid"')
                attrs.append(f'svg:stroke-color="{p["stroke_color"]}" svg:stroke-width="{p["sw"]:.3f}cm" '
                             f'draw:fill="none" draw:shadow="hidden"')
                if p["end"]:
                    attrs.append(f'draw:marker-end="Arrow" draw:marker-end-width="{p["head"]:.2f}cm" '
                                 f'draw:marker-end-center="false"')
                if p["start"]:
                    attrs.append(f'draw:marker-start="Arrow" draw:marker-start-width="{p["head"]:.2f}cm" '
                                 f'draw:marker-start-center="false"')
            out.append(f'<style:style style:name="{name}" style:family="graphic">'
                       f'<style:graphic-properties {" ".join(attrs)}/></style:style>')
        return "".join(out)

    def _pstyle_xml(self) -> str:
        out = []
        for key, name in self.pstyles.items():
            size, leading, color, bold, italic, align, before, after, hang, ls = key
            lh = size * leading * PT_TO_CM
            indent = (f' fo:margin-left="{hang:.3f}cm" fo:text-indent="-{hang:.3f}cm"' if hang else
                      ' fo:margin-left="0cm" fo:text-indent="0cm"')
            spacing = f' fo:letter-spacing="{ls:.2f}pt"' if ls else ""
            out.append(
                f'<style:style style:name="{name}" style:family="paragraph">'
                f'<style:paragraph-properties fo:line-height="{lh:.3f}cm" fo:text-align="{align}" '
                f'fo:margin-top="{before:.3f}cm" fo:margin-bottom="{after:.3f}cm"{indent} '
                f'style:writing-mode="lr-tb"/>'
                f'<style:text-properties style:font-name="{FONT_FAMILY}" fo:font-family="{FONT_FAMILY}" '
                f'fo:font-size="{size:.1f}pt" fo:color="{color}" '
                f'fo:font-weight="{"bold" if bold else "normal"}" '
                f'fo:font-style="{"italic" if italic else "normal"}" fo:language="en" fo:country="GB"'
                f'{spacing}/></style:style>')
        return "".join(out)

    def _tstyle_xml(self) -> str:
        out = []
        for key, name in self.tstyles.items():
            bold, italic, color, size = key
            col = f' fo:color="{color}"' if color else ""
            out.append(
                f'<style:style style:name="{name}" style:family="text"><style:text-properties '
                f'style:font-name="{FONT_FAMILY}" fo:font-family="{FONT_FAMILY}" fo:font-size="{size:.1f}pt" '
                f'fo:font-weight="{"bold" if bold else "normal"}" '
                f'fo:font-style="{"italic" if italic else "normal"}"{col}/></style:style>')
        return "".join(out)

    def save(self, path):
        content = (
            '<?xml version="1.0" encoding="UTF-8"?>'
            f'<office:document-content {NS} office:version="1.3">'
            f'<office:font-face-decls><style:font-face style:name="{FONT_FAMILY}" '
            f'svg:font-family="&apos;{FONT_FAMILY}&apos;" style:font-family-generic="swiss" '
            'style:font-pitch="variable"/></office:font-face-decls>'
            '<office:automatic-styles>'
            '<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties '
            'draw:background-size="border" draw:fill="solid" draw:fill-color="#FFFFFF"/></style:style>'
            f'{self._gstyle_xml()}{self._pstyle_xml()}{self._tstyle_xml()}'
            '</office:automatic-styles><office:body><office:drawing>'
            '<draw:page draw:name="A0 poster" draw:style-name="dp1" draw:master-page-name="Default">'
            f'{"".join(self.shapes)}</draw:page></office:drawing></office:body></office:document-content>')
        styles = (
            '<?xml version="1.0" encoding="UTF-8"?>'
            f'<office:document-styles {NS} office:version="1.3">'
            f'<office:font-face-decls><style:font-face style:name="{FONT_FAMILY}" '
            f'svg:font-family="&apos;{FONT_FAMILY}&apos;" style:font-family-generic="swiss" '
            'style:font-pitch="variable"/></office:font-face-decls>'
            '<office:styles>'
            '<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>'
            '<draw:stroke-dash draw:name="Dash" draw:style="rect" draw:dots1="1" '
            'draw:dots1-length="0.35cm" draw:distance="0.22cm"/>'
            '<style:default-style style:family="graphic"><style:graphic-properties draw:shadow="hidden"/>'
            f'<style:text-properties style:font-name="{FONT_FAMILY}" fo:font-family="{FONT_FAMILY}" '
            'fo:font-size="24pt" fo:language="en" fo:country="GB"/></style:default-style>'
            '</office:styles><office:automatic-styles>'
            '<style:page-layout style:name="PM1"><style:page-layout-properties '
            f'fo:page-width="{self.width:.1f}cm" fo:page-height="{self.height:.1f}cm" '
            'style:print-orientation="portrait" fo:margin-top="0cm" fo:margin-bottom="0cm" '
            'fo:margin-left="0cm" fo:margin-right="0cm"/></style:page-layout>'
            '<style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties '
            'draw:background-size="border" draw:fill="none"/></style:style>'
            '</office:automatic-styles><office:master-styles>'
            '<style:master-page style:name="Default" style:page-layout-name="PM1" draw:style-name="Mdp1"/>'
            '</office:master-styles></office:document-styles>')
        meta = (
            '<?xml version="1.0" encoding="UTF-8"?>'
            f'<office:document-meta {NS} office:version="1.3"><office:meta>'
            f'<dc:title>{escape(self.title)}</dc:title><dc:creator>{escape(self.creator)}</dc:creator>'
            f'<dc:description>{escape(self.description)}</dc:description>'
            '<meta:generator>AARNN poster_v2/build_poster.py</meta:generator>'
            '</office:meta></office:document-meta>')
        entries = "".join(
            f'<manifest:file-entry manifest:full-path="{p}" manifest:media-type="{mt}"/>'
            for p, (mt, _) in self.pictures.items())
        manifest = (
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" '
            'manifest:version="1.3">'
            '<manifest:file-entry manifest:full-path="/" manifest:version="1.3" '
            'manifest:media-type="application/vnd.oasis.opendocument.graphics"/>'
            '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
            '<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>'
            '<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>'
            f'{entries}</manifest:manifest>')
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr(zipfile.ZipInfo("mimetype"), "application/vnd.oasis.opendocument.graphics",
                       compress_type=zipfile.ZIP_STORED)
            for name, data in (("content.xml", content), ("styles.xml", styles), ("meta.xml", meta),
                               ("META-INF/manifest.xml", manifest)):
                z.writestr(name, data.encode("utf-8"), compress_type=zipfile.ZIP_DEFLATED)
            for p, (_, data) in self.pictures.items():
                z.writestr(p, data, compress_type=zipfile.ZIP_DEFLATED)
        with open(path, "wb") as f:
            f.write(buf.getvalue())


def _spaces(s: str) -> str:
    """Encode runs of 2+ spaces with <text:s/> so ODF keeps them."""
    out = []
    i = 0
    while i < len(s):
        if s[i] == " ":
            j = i
            while j < len(s) and s[j] == " ":
                j += 1
            n = j - i
            out.append(" " if n == 1 else f' <text:s text:c="{n - 1}"/>')
            i = j
        else:
            out.append(s[i])
            i += 1
    return "".join(out)


NS = (
    'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
    'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
    'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
    'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
    'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
    'xmlns:xlink="http://www.w3.org/1999/xlink" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" '
    'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"'
)
