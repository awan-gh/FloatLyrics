//! Direct2D/DirectWrite drawing into a per-pixel-alpha layered window.
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

type Pt = windows_numerics::Vector2;
fn pt(x: f32, y: f32) -> Pt { Pt { X: x, Y: y } }

pub fn color(s: &str) -> D2D1_COLOR_F {
    let h = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0xFFFFFFFF);
    let (a, rgb) = if h.len() == 8 { ((v >> 24) & 0xFF, v & 0xFFFFFF) } else if h.len() == 6 { (0xFF, v) } else { (0xFF, 0xFFFFFF) };
    D2D1_COLOR_F { r: ((rgb >> 16) & 0xFF) as f32 / 255.0, g: ((rgb >> 8) & 0xFF) as f32 / 255.0, b: (rgb & 0xFF) as f32 / 255.0, a: a as f32 / 255.0 }
}

pub fn valid_color(s: &str) -> bool {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6 || h.len() == 8) && u32::from_str_radix(h, 16).is_ok()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Fill { Solid, Gradient, Rainbow }

/// Subtitle-style text edge.
#[derive(Clone, Copy, PartialEq)]
pub enum Edge { None, Outline, Shadow, Raised, Depressed }

/// Background: one box around everything, a box behind each text line (caption style), or none.
#[derive(Clone, Copy, PartialEq)]
pub enum BgMode { None, Block, Line }

/// One row to draw.
pub struct Row<'a> {
    pub text: &'a str,
    pub sub: &'a str,
    pub size: f32,
    pub color: D2D1_COLOR_F,
    pub sub_color: D2D1_COLOR_F,
    /// characters sung (fractional); <= 0 means no wipe
    pub sung: f64,
    pub align: DWRITE_TEXT_ALIGNMENT,
    pub sub_scale: f32,
}

pub struct Style {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub edge: Edge,
    pub bg_mode: BgMode,
    pub outline: f32,
    pub outline_color: D2D1_COLOR_F,
    pub hl: D2D1_COLOR_F,
    pub hl2: D2D1_COLOR_F,
    pub fill: Fill,
    pub bg: D2D1_COLOR_F,
    pub frame: bool,
    pub opacity: f32,
    pub padding: f32,
    /// animation: vertical offset (px) and content alpha
    pub slide: f32,
    pub fade: f32,
    /// seconds, drives the rainbow motion
    pub clock: f64,
}

pub struct Renderer {
    d2d: ID2D1Factory,
    pub dw: IDWriteFactory5,
    custom: Option<IDWriteFontCollection1>,
    rt: ID2D1DCRenderTarget,
    memdc: HDC,
    dib: HBITMAP,
    size: (i32, i32),
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

/// char index -> UTF-16 index (DirectWrite positions are UTF-16)
fn u16_index(s: &str, chars: usize) -> u32 { s.chars().take(chars).map(|c| c.len_utf16() as u32).sum() }

impl Renderer {
    pub fn new() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0, dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = d2d.CreateDCRenderTarget(&props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE); // ClearType needs an opaque target
            let memdc = CreateCompatibleDC(None);
            Ok(Self { d2d, dw, custom: None, rt, memdc, dib: HBITMAP::default(), size: (0, 0) })
        }
    }

    /// Load every font file in `dir` (no install needed). Returns family names found there.
    pub fn load_fonts(&mut self, dir: &std::path::Path) -> Vec<String> {
        self.custom = None;
        let files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("ttf" | "otf" | "ttc")))
            .collect();
        if files.is_empty() { return vec![]; }
        unsafe {
            let Ok(builder) = self.dw.CreateFontSetBuilder() else { return vec![] };
            let Ok(builder) = builder.cast::<IDWriteFontSetBuilder1>() else { return vec![] };
            for f in &files {
                let w: Vec<u16> = f.to_string_lossy().encode_utf16().chain(Some(0)).collect();
                if let Ok(file) = self.dw.CreateFontFileReference(PCWSTR(w.as_ptr()), None) { let _ = builder.AddFontFile(&file); }
            }
            let Ok(set) = builder.CreateFontSet() else { return vec![] };
            let Ok(coll) = self.dw.CreateFontCollectionFromFontSet(&set) else { return vec![] };
            let names = family_names(&coll);
            self.custom = Some(coll);
            names
        }
    }

    pub fn system_fonts(&self) -> Vec<String> {
        unsafe {
            let mut c: Option<IDWriteFontCollection1> = None;
            if self.dw.GetSystemFontCollection(false, &mut c, false).is_err() { return vec![]; }
            c.map(|c| family_names(&c)).unwrap_or_default()
        }
    }

    fn format(&self, st: &Style, size: f32, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
        unsafe {
            let fam = HSTRING::from(st.family.as_str());
            // custom collection first if it has the family, else system
            let coll: Option<IDWriteFontCollection> = self.custom.as_ref().and_then(|c| {
                let mut idx = 0; let mut exists = BOOL(0);
                c.FindFamilyName(&fam, &mut idx, &mut exists).ok()?;
                if exists.as_bool() { c.cast().ok() } else { None }
            });
            let f = self.dw.CreateTextFormat(&fam, coll.as_ref(), if st.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                if st.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL }, DWRITE_FONT_STRETCH_NORMAL, size.max(4.0), w!("en-us"))?;
            f.SetTextAlignment(align)?;
            Ok(f)
        }
    }

    fn layout(&self, text: &str, fmt: &IDWriteTextFormat, width: f32) -> Result<(IDWriteTextLayout, f32)> {
        unsafe {
            let w = wide(if text.is_empty() { " " } else { text });
            let l = self.dw.CreateTextLayout(&w, fmt, width.max(1.0), 10_000.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            l.GetMetrics(&mut m)?;
            Ok((l, m.height))
        }
    }

    /// Lay out rows, size the window to fit, draw, push to screen. Returns the window height in px.
    pub fn draw(&mut self, hwnd: HWND, width: i32, rows: &[Row], st: &Style, fixed_height: Option<i32>) -> Result<i32> {
        let pad = st.padding + st.outline;
        let inner = width as f32 - 2.0 * pad;
        let mut laid = Vec::with_capacity(rows.len());
        let mut y = pad;
        for r in rows {
            let (main, mh) = self.layout(r.text, &self.format(st, r.size, r.align)?, inner)?;
            let sub = if r.sub.is_empty() { None } else { Some(self.layout(r.sub, &self.format(st, r.size * r.sub_scale, r.align)?, inner)?) };
            let top = y;
            y += mh + sub.as_ref().map(|s| s.1).unwrap_or(0.0);
            laid.push((top, mh, main, sub));
        }
        let height = fixed_height.unwrap_or((y + pad).ceil() as i32).max(1);
        unsafe {
            self.ensure_surface(width, height)?;
            let rt = &self.rt;
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let rect = D2D_RECT_F { left: 0.5, top: 0.5, right: width as f32 - 0.5, bottom: height as f32 - 0.5 };
            let rr = D2D1_ROUNDED_RECT { rect, radiusX: 10.0, radiusY: 10.0 };
            if st.bg.a > 0.0 && st.bg_mode == BgMode::Block { rt.FillRoundedRectangle(&rr, &rt.CreateSolidColorBrush(&st.bg, None)?); }
            if st.frame { rt.DrawRoundedRectangle(&rr, &rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.6 }, None)?, 1.0, None); }

            rt.PushAxisAlignedClip(&D2D_RECT_F { left: 0.0, top: 0.0, right: width as f32, bottom: height as f32 }, D2D1_ANTIALIAS_MODE_ALIASED);
            let a = st.opacity * st.fade;
            let outline = rt.CreateSolidColorBrush(&st.outline_color, None)?; outline.SetOpacity(a);
            for (row, (top, mh, main, sub)) in rows.iter().zip(&laid) {
                let origin = pt(pad, top + st.slide);
                let sub_origin = pt(pad, top + mh + st.slide);
                if st.bg_mode == BgMode::Line && st.bg.a > 0.0 && !row.text.trim().is_empty() {
                    // caption style: a box hugging each wrapped text line
                    let bgb = rt.CreateSolidColorBrush(&st.bg, None)?; bgb.SetOpacity(a);
                    self.line_boxes(main, row.text, origin, row.size * 0.25, &bgb)?;
                    if let Some((sl, _)) = sub { self.line_boxes(sl, row.sub, sub_origin, row.size * 0.15, &bgb)?; }
                }
                let base = rt.CreateSolidColorBrush(&row.color, None)?; base.SetOpacity(a);
                self.text_edged(main, origin, &base, &outline, st.outline, st.edge);
                if let Some((sl, _)) = sub {
                    let sb = rt.CreateSolidColorBrush(&row.sub_color, None)?; sb.SetOpacity(a);
                    self.text_edged(sl, sub_origin, &sb, &outline, st.outline * 0.7, st.edge);
                }
                if row.sung > 0.0 && !row.text.is_empty() {
                    let hl = self.highlight_brush(st, width as f32)?; hl.SetOpacity(a);
                    self.wipe(main, row.text, row.sung, origin, &hl)?;
                }
            }
            rt.PopAxisAlignedClip();
            rt.EndDraw(None, None)?;

            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let size = SIZE { cx: width, cy: height };
            let _ = UpdateLayeredWindow(hwnd, None, None, Some(&size), Some(self.memdc), Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
        Ok(height)
    }

    unsafe fn text_edged(&self, l: &IDWriteTextLayout, o: Pt, fill: &ID2D1SolidColorBrush, edge: &ID2D1SolidColorBrush, w: f32, mode: Edge) {
        let stamp = |dx: f32, dy: f32| self.rt.DrawTextLayout(pt(o.X + dx, o.Y + dy), l, edge, D2D1_DRAW_TEXT_OPTIONS_NONE);
        if w > 0.0 {
            match mode {
                Edge::None => {}
                Edge::Outline => {
                    // shortcut: outline = text stamped around a circle; fine up to ~10 px, a glyph-geometry stroke would be exact
                    let n = if w > 3.0 { 16 } else { 8 };
                    for k in 0..n { let t = k as f32 / n as f32 * std::f32::consts::TAU; stamp(w * t.cos(), w * t.sin()); }
                }
                Edge::Shadow => { let n = (w * 2.0).ceil() as i32; for k in 1..=n { let d = k as f32 * 0.75; stamp(d, d); } }
                Edge::Raised => { for k in 1..=w.ceil() as i32 { stamp(k as f32 * 0.6, k as f32 * 0.6); } }
                Edge::Depressed => { for k in 1..=w.ceil() as i32 { stamp(-(k as f32) * 0.6, -(k as f32) * 0.6); } }
            }
        }
        self.rt.DrawTextLayout(o, l, fill, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }

    unsafe fn line_boxes(&self, l: &IDWriteTextLayout, text: &str, o: Pt, pad: f32, brush: &ID2D1SolidColorBrush) -> Result<()> {
        // merge hit-test rects that share a visual line
        let mut lines: Vec<(f32, f32, f32, f32)> = Vec::new();
        for (x, y, w, h) in hit_rects(l, 0, u16_index(text, text.chars().count()))? {
            match lines.iter_mut().find(|r| (r.1 - y).abs() < 0.5) {
                Some(r) => { let right = (r.0 + r.2).max(x + w); r.0 = r.0.min(x); r.2 = right - r.0; }
                None => lines.push((x, y, w, h)),
            }
        }
        for (x, y, w, h) in lines {
            let rect = D2D_RECT_F { left: o.X + x - pad, top: o.Y + y, right: o.X + x + w + pad, bottom: o.Y + y + h };
            self.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: pad * 0.6, radiusY: pad * 0.6 }, brush);
        }
        Ok(())
    }

    unsafe fn highlight_brush(&self, st: &Style, width: f32) -> Result<ID2D1Brush> {
        let rt = &self.rt;
        Ok(match st.fill {
            Fill::Solid => rt.CreateSolidColorBrush(&st.hl, None)?.cast()?,
            Fill::Gradient | Fill::Rainbow => {
                let stops: Vec<D2D1_GRADIENT_STOP> = if st.fill == Fill::Gradient {
                    vec![D2D1_GRADIENT_STOP { position: 0.0, color: st.hl }, D2D1_GRADIENT_STOP { position: 1.0, color: st.hl2 }]
                } else {
                    (0..=6).map(|i| D2D1_GRADIENT_STOP { position: i as f32 / 6.0, color: hue(i as f32 / 6.0) }).collect()
                };
                let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_MIRROR)?;
                // rainbow drifts sideways over time
                let shift = if st.fill == Fill::Rainbow { (st.clock * 0.25).fract() as f32 * width * 2.0 } else { 0.0 };
                let span = if st.fill == Fill::Rainbow { width * 0.6 } else { width };
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(shift, 0.0), endPoint: pt(shift + span, 0.0) };
                rt.CreateLinearGradientBrush(&props, None, &coll)?.cast()?
            }
        })
    }

    /// Draw the sung part: clip to the hit-test rects of the sung characters (handles wrapped lines).
    unsafe fn wipe(&self, l: &IDWriteTextLayout, text: &str, sung: f64, o: Pt, brush: &ID2D1Brush) -> Result<()> {
        let total = text.chars().count();
        let full = (sung.floor() as usize).min(total);
        let mut rects = Vec::new();
        if full > 0 { rects.extend(hit_rects(l, 0, u16_index(text, full))?); }
        if full < total {
            let frac = (sung - full as f64) as f32;
            let start = u16_index(text, full);
            let len = u16_index(text, full + 1) - start;
            for mut r in hit_rects(l, start, len)? { r.2 *= frac; rects.push(r); }
        }
        for (x, y, w, h) in rects {
            self.rt.PushAxisAlignedClip(&D2D_RECT_F { left: o.X + x - 20.0 * (x == 0.0) as i32 as f32, top: o.Y + y - 20.0, right: o.X + x + w, bottom: o.Y + y + h + 20.0 }, D2D1_ANTIALIAS_MODE_ALIASED);
            self.rt.DrawTextLayout(o, l, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
            self.rt.PopAxisAlignedClip();
        }
        Ok(())
    }

    unsafe fn ensure_surface(&mut self, w: i32, h: i32) -> Result<()> {
        if self.size == (w, h) { return Ok(()); }
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(self.memdc), &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(self.memdc, dib.into());
        if !self.dib.is_invalid() { let _ = DeleteObject(self.dib.into()); }
        self.dib = dib;
        self.size = (w, h);
        self.rt.BindDC(self.memdc, &RECT { left: 0, top: 0, right: w, bottom: h })?;
        Ok(())
    }
}

unsafe fn hit_rects(l: &IDWriteTextLayout, start: u32, len: u32) -> Result<Vec<(f32, f32, f32, f32)>> {
    if len == 0 { return Ok(vec![]); }
    let mut count = 0u32;
    let _ = l.HitTestTextRange(start, len, 0.0, 0.0, None, &mut count);
    let mut m = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
    l.HitTestTextRange(start, len, 0.0, 0.0, Some(&mut m), &mut count)?;
    Ok(m.iter().take(count as usize).map(|h| (h.left, h.top, h.width, h.height)).collect())
}

fn hue(t: f32) -> D2D1_COLOR_F {
    let h = t * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as i32 { 0 => (1.0, x, 0.0), 1 => (x, 1.0, 0.0), 2 => (0.0, 1.0, x), 3 => (0.0, x, 1.0), 4 => (x, 0.0, 1.0), _ => (1.0, 0.0, x) };
    D2D1_COLOR_F { r, g, b, a: 1.0 }
}

unsafe fn family_names(c: &IDWriteFontCollection1) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..c.GetFontFamilyCount() {
        let Ok(fam) = c.GetFontFamily(i) else { continue };
        let Ok(names) = fam.GetFamilyNames() else { continue };
        let mut idx = 0; let mut exists = BOOL(0);
        let _ = names.FindLocaleName(w!("en-us"), &mut idx, &mut exists);
        if !exists.as_bool() { idx = 0; }
        let len = names.GetStringLength(idx).unwrap_or(0);
        let mut buf = vec![0u16; len as usize + 1];
        if names.GetString(idx, &mut buf).is_ok() { out.push(String::from_utf16_lossy(&buf[..len as usize])); }
    }
    out.sort();
    out.dedup();
    out
}

pub const D2D1_COLOR_F_CLEAR: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
/// 1/255 alpha: invisible but still hit-testable (layered windows ignore clicks on alpha-0 pixels)
pub const D2D1_COLOR_F_TRANSPARENT_HIT: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 / 255.0 };

impl Renderer {
    /// Render `text` in the font from `file` to a premultiplied RGBA buffer (for previews in the settings window).
    pub fn preview(&self, file: &std::path::Path, family: &str, text: &str, px: f32, fill: D2D1_COLOR_F, outline: D2D1_COLOR_F, edge: f32) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        unsafe {
            let builder = self.dw.CreateFontSetBuilder().map_err(|e| format!("line {}: {e}", line!()))?.cast::<IDWriteFontSetBuilder1>().map_err(|e| format!("line {}: {e}", line!()))?;
            let w: Vec<u16> = file.to_string_lossy().encode_utf16().chain(Some(0)).collect();
            builder.AddFontFile(&self.dw.CreateFontFileReference(PCWSTR(w.as_ptr()), None).map_err(|e| format!("line {}: {e}", line!()))?).map_err(|e| format!("line {}: {e}", line!()))?;
            let coll = self.dw.CreateFontCollectionFromFontSet(&builder.CreateFontSet().map_err(|e| format!("line {}: {e}", line!()))?).map_err(|e| format!("line {}: {e}", line!()))?;
            let coll: IDWriteFontCollection = coll.cast().map_err(|e| format!("line {}: {e}", line!()))?;
            let fmt = self.dw.CreateTextFormat(&HSTRING::from(family), &coll, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, px, w!("en-us")).map_err(|e| format!("line {}: {e}", line!()))?;
            let ws = wide(text);
            let layout = self.dw.CreateTextLayout(&ws, &fmt, 4000.0, 1000.0).map_err(|e| format!("line {}: {e}", line!()))?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m).map_err(|e| format!("line {}: {e}", line!()))?;
            let (wd, ht) = ((m.width + 2.0 * edge + 4.0).ceil() as i32, (m.height + 2.0 * edge + 2.0).ceil() as i32);
            if wd <= 0 || ht <= 0 { return Err("empty".into()); }

            let dc = CreateCompatibleDC(None);
            let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: wd, biHeight: -ht, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).map_err(|e| format!("line {}: {e}", line!()))?;
            let old = SelectObject(dc, dib.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT, pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT };
            let out = (|| -> std::result::Result<Vec<u8>, String> {
                let rt = self.d2d.CreateDCRenderTarget(&props).map_err(|e| format!("line {}: {e}", line!()))?;
                rt.BindDC(dc, &RECT { left: 0, top: 0, right: wd, bottom: ht }).map_err(|e| format!("line {}: {e}", line!()))?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                rt.BeginDraw();
                rt.Clear(Some(&D2D1_COLOR_F_CLEAR));
                let o = pt(edge + 2.0, edge + 1.0);
                let ob = rt.CreateSolidColorBrush(&outline, None).map_err(|e| format!("line {}: {e}", line!()))?;
                if edge > 0.0 { for k in 0..8 { let t = k as f32 / 8.0 * std::f32::consts::TAU; rt.DrawTextLayout(pt(o.X + edge * t.cos(), o.Y + edge * t.sin()), &layout, &ob, D2D1_DRAW_TEXT_OPTIONS_NONE); } }
                rt.DrawTextLayout(o, &layout, &rt.CreateSolidColorBrush(&fill, None).map_err(|e| format!("line {}: {e}", line!()))?, D2D1_DRAW_TEXT_OPTIONS_NONE);
                rt.EndDraw(None, None).map_err(|e| format!("line {}: {e}", line!()))?;
                // BGRA -> RGBA (both premultiplied)
                let src = std::slice::from_raw_parts(bits as *const u8, (wd * ht * 4) as usize);
                Ok(src.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect())
            })();
            SelectObject(dc, old);
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(dc);
            out.map(|v| (wd as u32, ht as u32, v))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore] // needs a downloaded preview subset: run the gfonts live test first
    fn preview_renders() {
        let dir = crate::settings::Settings::fonts_dir().join(".preview");
        let f = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "ttf")).unwrap();
        let r = super::Renderer::new().unwrap();
        let out = r.preview(&f, "Roboto", "Roboto", 24.0, super::color("#FFFFFFFF"), super::color("#00000000"), 0.0);
        println!("{f:?} -> {:?}", out.as_ref().map(|o| (o.0, o.1)));
        assert!(out.is_ok());
    }
}
