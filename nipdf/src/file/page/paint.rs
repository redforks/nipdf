use std::{
    borrow::Cow, collections::HashMap, convert::AsRef, fs::File, io::Read, ops::RangeInclusive,
};

use crate::{
    file::{ObjectResolver, XObjectDict},
    function::{Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        parse_operations, AxialExtend, AxialShadingDict, ColorArgs, ColorArgsOrName, LineCapStyle,
        LineJoinStyle, NameOfDict, NameOrDictByRef, NameOrStream, PatternType, Point,
        RenderingIntent, ShadingPatternDict, ShadingType, TextRenderingMode, TilingPaintType,
        TilingPatternDict,
    },
    object::{Object, PdfObject, Stream, TextStringOrNumber},
    text::{
        CIDFontType, CIDFontWidths, Encoding, EncodingDict, FontDescriptorDict,
        FontDescriptorFlags, FontDict, FontType, Type0FontDict, Type1FontDict,
    },
};
use anyhow::{anyhow, Ok, Result as AnyResult};
use cff_parser::{File as CffFile, Font as CffFont};
use educe::Educe;
use font_kit::loaders::freetype::Font as FontKitFont;
use fontdb::{Database, Family, Query, Source, Weight};
use image::RgbaImage;
use itertools::Either;
use log::{debug, error, info, warn};
use nom::{combinator::eof, sequence::terminated};
use once_cell::sync::Lazy;
use pathfinder_geometry::{line_segment::LineSegment2F, vector::Vector2F};
use ttf_parser::{Face as TTFFace, GlyphId, OutlineBuilder};

use super::{GraphicsStateParameterDict, Operation, Rectangle, ResourceDict};
use crate::graphics::color_space;
use crate::graphics::color_space::ColorSpace;

use crate::graphics::trans::{
    image_to_device_space, move_text_space_pos, move_text_space_right, to_device_space,
    ImageToDeviceSpace, IntoSkiaTransform, TextToUserSpace, UserToDeviceIndependentSpace,
    UserToDeviceSpace,
};
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, Mask, MaskType, Paint, Path as SkiaPath, PathBuilder,
    Pixmap, PixmapPaint, PixmapRef, Point as SkiaPoint, Rect, Stroke, StrokeDash, Transform,
};

impl From<LineCapStyle> for tiny_skia::LineCap {
    fn from(cap: LineCapStyle) -> Self {
        match cap {
            LineCapStyle::Butt => tiny_skia::LineCap::Butt,
            LineCapStyle::Round => tiny_skia::LineCap::Round,
            LineCapStyle::Square => tiny_skia::LineCap::Square,
        }
    }
}

impl From<LineJoinStyle> for tiny_skia::LineJoin {
    fn from(join: LineJoinStyle) -> Self {
        match join {
            LineJoinStyle::Miter => tiny_skia::LineJoin::Miter,
            LineJoinStyle::Round => tiny_skia::LineJoin::Round,
            LineJoinStyle::Bevel => tiny_skia::LineJoin::Bevel,
        }
    }
}

impl From<Point> for SkiaPoint {
    fn from(p: Point) -> Self {
        Self::from_xy(p.x, p.y)
    }
}

#[derive(Debug, Clone)]
enum PaintCreator {
    Color(tiny_skia::Color),
    Gradient(Paint<'static>),
    Tile((Pixmap, UserToDeviceIndependentSpace)),
}

impl PaintCreator {
    fn create(&self) -> Cow<'_, Paint<'_>> {
        match self {
            PaintCreator::Color(c) => {
                let mut r = Paint::default();
                r.set_color(*c);
                Cow::Owned(r)
            }

            PaintCreator::Gradient(p) => Cow::Borrowed(p),

            PaintCreator::Tile((p, matrix)) => {
                let mut r = Paint::default();
                let height = p.height() as f32;
                let transform = to_device_space(height, 1.0, *matrix);
                r.shader = tiny_skia::Pattern::new(
                    p.as_ref(),
                    tiny_skia::SpreadMode::Repeat,
                    FilterQuality::Bicubic,
                    1.0f32,
                    transform.into_skia(),
                );
                Cow::Owned(r)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct State {
    width: f32,
    height: f32,
    zoom: f32,
    /// ctm get from pdf file
    ctm: UserToDeviceIndependentSpace,
    /// ctm with flip_y and zoom
    user_to_device: UserToDeviceSpace,
    fill_paint: PaintCreator,
    stroke_paint: PaintCreator,
    stroke: Stroke,
    mask: Option<Mask>,
    fill_color_space: Box<dyn ColorSpace<f32>>,
    stroke_color_space: Box<dyn ColorSpace<f32>>,
    text_object: TextObject,
}

impl State {
    /// height: height in user space coordinate
    fn new(option: &RenderOption) -> Self {
        let mut r = Self {
            zoom: option.zoom,
            width: option.width as f32,
            height: option.height as f32,
            user_to_device: to_device_space(
                option.height as f32,
                option.zoom,
                UserToDeviceIndependentSpace::identity(),
            ),
            ctm: UserToDeviceIndependentSpace::identity(),
            fill_paint: PaintCreator::Color(tiny_skia::Color::BLACK),
            stroke_paint: PaintCreator::Color(tiny_skia::Color::BLACK),
            stroke: Stroke::default(),
            mask: None,
            fill_color_space: Box::new(color_space::DeviceRGB()),
            stroke_color_space: Box::new(color_space::DeviceRGB()),
            text_object: TextObject::new(),
        };

        r.set_line_cap(LineCapStyle::default());
        r.set_line_join(LineJoinStyle::default());
        r.set_miter_limit(10.0);
        r.set_dash_pattern(&[], 0.0);
        r.set_render_intent(RenderingIntent::default());

        r
    }

    fn set_line_width(&mut self, w: f32) {
        self.stroke.width = w;
    }

    fn set_line_cap(&mut self, cap: LineCapStyle) {
        self.stroke.line_cap = cap.into();
    }

    fn set_line_join(&mut self, join: LineJoinStyle) {
        self.stroke.line_join = join.into();
    }

    fn set_dash_pattern(&mut self, pattern: &[f32], phase: f32) {
        self.stroke.dash = StrokeDash::new(pattern.to_owned(), phase);
    }

    fn set_miter_limit(&mut self, limit: f32) {
        self.stroke.miter_limit = limit;
    }

    fn set_flatness(&mut self, flatness: f32) {
        info!("not implemented: flatness: {}", flatness);
    }

    fn set_render_intent(&mut self, intent: RenderingIntent) {
        info!("not implemented: render intent: {}", intent);
    }

    fn set_stroke_color_and_space(&mut self, cs: Box<dyn ColorSpace<f32>>, color: &[f32]) {
        self.set_stroke_color(cs.to_skia_color(color));
        self.stroke_color_space = cs;
    }

    fn set_stroke_color(&mut self, color: tiny_skia::Color) {
        debug!("set stroke color: {:?}", color);
        self.stroke_paint = PaintCreator::Color(color);
    }

    fn set_stroke_color_args(&mut self, args: ColorArgs) {
        let color = self.stroke_color_space.to_skia_color(args.as_ref());
        self.set_stroke_color(color);
    }

    fn set_fill_color_and_space(&mut self, cs: Box<dyn ColorSpace<f32>>, color: &[f32]) {
        self.set_fill_color(cs.to_skia_color(color));
        self.fill_color_space = cs;
    }

    fn set_fill_color(&mut self, color: tiny_skia::Color) {
        debug!("set fill color: {:?}", color);
        self.fill_paint = PaintCreator::Color(color);
    }

    fn set_fill_color_args(&mut self, args: ColorArgs) {
        let color = self.fill_color_space.to_skia_color(args.as_ref());
        self.set_fill_color(color);
    }

    fn concat_ctm(&mut self, ctm: UserToDeviceIndependentSpace) {
        self.ctm = self.ctm.then(&ctm.with_source());
        self.user_to_device = to_device_space(self.height, self.zoom, self.ctm);
    }

    fn get_fill_paint(&self) -> Cow<'_, Paint<'_>> {
        self.fill_paint.create()
    }

    fn get_stroke_paint(&self) -> Cow<'_, Paint<'_>> {
        self.stroke_paint.create()
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> ImageToDeviceSpace {
        image_to_device_space(img_w, img_h, self.height, self.zoom, self.ctm)
    }

    fn get_mask(&self) -> Option<&Mask> {
        self.mask.as_ref()
    }

    fn set_graphics_state(&mut self, res: &GraphicsStateParameterDict) {
        for key in res.d.dict().keys() {
            match key.as_ref() {
                "LW" => self.set_line_width(res.line_width().unwrap().unwrap()),
                "LC" => self.set_line_cap(res.line_cap().unwrap().unwrap()),
                "LJ" => self.set_line_join(res.line_join().unwrap().unwrap()),
                "ML" => self.set_miter_limit(res.miter_limit().unwrap().unwrap()),
                "RI" => self.set_render_intent(res.rendering_intent().unwrap().unwrap()),
                "TK" => self.set_text_knockout_flag(res.text_knockout_flag().unwrap().unwrap()),
                "FL" => self.set_flatness(res.flatness().unwrap().unwrap()),
                "Type" => (),
                "SM" => debug!("ExtGState key: SM (smoothness tolerance) not implemented"),
                k @ ("OPM" | "op" | "OP") => {
                    debug!("ExtGState key {k} is for Overprint, which is not supported")
                }
                "SA" => {
                    debug!("Unknown or unsupported ExtGState key: SA (automatic stroke adjustment)")
                }
                _ => info!("Unknown or unsupported ExtGState key: {}", key.as_ref()),
            }
        }
    }

    fn new_mask(&self) -> Mask {
        let w = self.width * self.zoom;
        let h = self.height * self.zoom;
        let mut r = Mask::new(w as u32, h as u32).unwrap();
        let p = PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, w, h).unwrap());
        r.fill_path(&p, FillRule::Winding, true, Transform::identity());
        r
    }

    fn update_mask(&mut self, path: &SkiaPath, rule: FillRule, flip_y: bool) {
        let mut mask = self.mask.take().unwrap_or_else(|| self.new_mask());
        let transform = if flip_y {
            to_device_space(self.height, self.zoom, self.ctm).into_skia()
        } else {
            Transform::identity()
        };
        mask.intersect_path(path, rule, true, transform);
        self.mask = Some(mask);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Winding fill rule.
    fn clip_non_zero(&mut self, path: &SkiaPath, flip_y: bool) {
        self.update_mask(path, FillRule::Winding, flip_y);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Even-Odd fill rule.
    fn clip_even_odd(&mut self, path: &SkiaPath, flip_y: bool) {
        self.update_mask(path, FillRule::EvenOdd, flip_y);
    }

    fn set_text_knockout_flag(&mut self, knockout: bool) {
        self.text_object.knockout = knockout;
    }

    pub fn end_text_object(&mut self) {
        // if exists text clipping path, intersection to current clipping path using Winding fill rule
        let p = self.text_object.text_clipping_path.finish();
        if let Some(p) = p {
            let p = p.to_owned();
            self.clip_non_zero(&p, false);
            self.text_object.text_clipping_path.reset();
        }
    }
}

#[derive(Debug, Clone)]
struct Path {
    path: Either<PathBuilder, SkiaPath>,
}

impl Default for Path {
    fn default() -> Self {
        Self {
            path: Either::Left(PathBuilder::new()),
        }
    }
}

impl Path {
    fn path_builder(&mut self) -> &mut PathBuilder {
        self.path.as_mut().left().unwrap()
    }

    pub fn close_path(&mut self) {
        self.path_builder().close();
    }

    pub fn move_to(&mut self, p: Point) {
        self.path_builder().move_to(p.x, p.y);
    }

    pub fn line_to(&mut self, p: Point) {
        self.path_builder().line_to(p.x, p.y);
    }

    pub fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path_builder()
            .cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    }

    pub fn curve_to_cur_point_as_control(&mut self, p2: Point, p3: Point) {
        let p1 = self.path_builder().last_point().unwrap();
        self.curve_to(Point { x: p1.x, y: p1.y }, p2, p3);
    }

    pub fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) {
        self.curve_to(p1, p3, p3);
    }

    pub fn append_rect(&mut self, p: Point, w: f32, h: f32) {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path_builder().push_rect(r.into());
    }

    /// Build path and clear the path builder, return None if path is empty
    pub fn finish(&mut self) -> Option<&SkiaPath> {
        if let Either::Left(_) = self.path {
            let temp = Either::Left(PathBuilder::new());
            let pb = std::mem::replace(&mut self.path, temp).left().unwrap();
            if let Some(p) = pb.finish() {
                self.path = Either::Right(p);
            } else {
                debug!("empty or invalid path");
            }
        }

        match &self.path {
            Either::Left(_) => None,
            Either::Right(p) => Some(p),
        }
    }

    pub fn reset(&mut self) {
        let temp = Either::Left(PathBuilder::new());
        let p = std::mem::replace(&mut self.path, temp);
        self.path = p.right_and_then(|p| Either::Left(p.clear()));
    }

    pub fn clear(&mut self) {
        self.reset();
    }
}

/// Option for Render
#[derive(Debug, Educe)]
#[educe(Default)]
pub struct RenderOption {
    /// zoom level default to 1.0
    #[educe(Default = 1.0)]
    zoom: f32,
    width: u32,
    height: u32,
    /// If crop is specified, the output canvas will be cropped to the specified rectangle.
    crop: Option<Rectangle>,
}

#[derive(Educe)]
#[educe(Default(new))]
pub struct RenderOptionBuilder(RenderOption);

impl RenderOptionBuilder {
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.0.zoom = zoom;
        self
    }

    pub fn width(mut self, width: u32) -> Self {
        self.0.width = width;
        self
    }

    pub fn height(mut self, height: u32) -> Self {
        self.0.height = height;
        self
    }

    pub fn crop(mut self, rect: Option<Rectangle>) -> Self {
        self.0.crop = rect;
        self
    }

    pub fn build(self) -> RenderOption {
        self.0
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Render<'a, 'b, 'c> {
    canvas: Pixmap,
    stack: Vec<State>,
    width: u32,
    height: u32,
    path: Path,
    #[educe(Debug(ignore))]
    font_cache: FontCache<'c>,
    resources: &'c ResourceDict<'a, 'b>,
    crop: Option<Rectangle>,
}

impl<'a, 'b, 'c> Render<'a, 'b, 'c> {
    pub fn new(option: RenderOption, resources: &'c ResourceDict<'a, 'b>) -> Self
    where
        'a: 'c,
        'b: 'c,
    {
        let w = (option.width as f32 * option.zoom) as u32;
        let h = (option.height as f32 * option.zoom) as u32;

        let mut canvas = Pixmap::new(w, h).unwrap();
        // fill the whole canvas with white
        canvas.fill(tiny_skia::Color::WHITE);
        Self {
            canvas,
            stack: vec![State::new(&option)],
            width: option.width,
            height: option.height,
            path: Path::default(),
            font_cache: FontCache::new(resources).unwrap(),
            resources,
            crop: option.crop,
        }
    }

    fn device_width(&self) -> u32 {
        self.canvas.width()
    }

    fn device_height(&self) -> u32 {
        self.canvas.height()
    }

    fn push(&mut self) {
        self.stack.push(self.stack.last().unwrap().clone());
    }

    fn pop(&mut self) {
        self.stack.pop().unwrap();
    }

    fn current_mut(&mut self) -> &mut State {
        self.stack.last_mut().unwrap()
    }

    pub fn into(self) -> Pixmap {
        let r = self.canvas;
        // crop the canvas if crop is specified
        if let Some(rect) = self.crop {
            let state = self.stack.last().unwrap();
            let transform = to_device_space(
                state.height,
                state.zoom,
                UserToDeviceIndependentSpace::identity(),
            );
            let p = transform.transform_point((rect.left_x, rect.upper_y).into());
            let mut canvas = Pixmap::new(
                (rect.width() * state.zoom) as u32,
                (rect.height() * state.zoom) as u32,
            )
            .unwrap();
            canvas.draw_pixmap(
                -p.x as i32,
                -p.y as i32,
                r.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            canvas
        } else {
            r
        }
    }

    fn text_object(&self) -> &TextObject {
        &self.stack.last().unwrap().text_object
    }

    fn text_object_mut(&mut self) -> &mut TextObject {
        &mut self.current_mut().text_object
    }

    pub(crate) fn exec(&mut self, op: Operation<'_>) {
        debug!("handle operation: {:?}", op);
        match op {
            // General Graphics State Operations
            Operation::SetLineWidth(width) => self.current_mut().set_line_width(width),
            Operation::SetLineCap(cap) => self.current_mut().set_line_cap(cap),
            Operation::SetLineJoin(join) => self.current_mut().set_line_join(join),
            Operation::SetMiterLimit(limit) => self.current_mut().set_miter_limit(limit),
            Operation::SetDashPattern(pattern, phase) => {
                self.current_mut().set_dash_pattern(&pattern, phase)
            }
            Operation::SetRenderIntent(intent) => self.current_mut().set_render_intent(intent),
            Operation::SetFlatness(flatness) => self.current_mut().set_flatness(flatness),
            Operation::SetGraphicsStateParameters(nm) => {
                let res = self.resources.ext_g_state().unwrap();
                let res = res.get(&nm.0).expect("ExtGState not found");
                self.current_mut().set_graphics_state(res);
            }

            // Special Graphics State Operations
            Operation::SaveGraphicsState => self.push(),
            Operation::RestoreGraphicsState => self.pop(),
            Operation::ModifyCTM(ctm) => self.current_mut().concat_ctm(ctm),

            // Path Construction Operations
            Operation::MoveToNext(p) => self.path.move_to(p),
            Operation::LineToNext(p) => self.path.line_to(p),
            Operation::AppendBezierCurve(p1, p2, p3) => self.path.curve_to(p1, p2, p3),
            Operation::AppendBezierCurve2(p2, p3) => {
                self.path.curve_to_cur_point_as_control(p2, p3);
            }
            Operation::AppendBezierCurve1(p1, p3) => {
                self.path.curve_to_dest_point_as_control(p1, p3);
            }
            Operation::ClosePath => self.path.close_path(),
            Operation::AppendRectangle(p, w, h) => self.path.append_rect(p, w, h),

            // Path Painting Operation
            Operation::Stroke => self.stroke(),
            Operation::CloseAndStroke => self.close_and_stroke(),
            Operation::FillNonZero | Operation::FillNonZeroDeprecated => self.fill_path_non_zero(),
            Operation::FillEvenOdd => self.fill_path_even_odd(),
            Operation::FillAndStrokeNonZero => self.fill_and_stroke_non_zero(),
            Operation::FillAndStrokeEvenOdd => self.fill_and_stroke_even_odd(),
            Operation::CloseFillAndStrokeNonZero => self.close_fill_and_stroke_non_zero(),
            Operation::CloseFillAndStrokeEvenOdd => self.close_fill_and_stroke_even_odd(),
            Operation::EndPath => self.end_path(),

            // Clipping Path Operations
            Operation::ClipNonZero => {
                let state = self.stack.last_mut().unwrap();
                if let Some(p) = self.path.finish() {
                    state.clip_non_zero(p, true);
                }
            }
            Operation::ClipEvenOdd => {
                let state = self.stack.last_mut().unwrap();
                if let Some(p) = self.path.finish() {
                    state.clip_even_odd(p, true);
                }
            }

            // Text Object Operations
            Operation::BeginText => self.text_object_mut().reset(),
            Operation::EndText => self.end_text(),

            // Text State Operations
            Operation::SetCharacterSpacing(spacing) => {
                self.text_object_mut().set_character_spacing(spacing);
            }
            Operation::SetWordSpacing(spacing) => self.text_object_mut().set_word_spacing(spacing),
            Operation::SetHorizontalScaling(scale) => {
                self.text_object_mut().set_horizontal_scaling(scale);
            }
            Operation::SetLeading(leading) => self.text_object_mut().set_leading(leading),
            Operation::SetFont(name, size) => self.text_object_mut().set_font(&name, size),
            Operation::SetTextRenderingMode(mode) => {
                self.text_object_mut().set_text_rendering_mode(mode);
            }
            Operation::SetTextRise(rise) => self.text_object_mut().set_text_rise(rise),

            // Text Positioning Operations
            Operation::MoveTextPosition(p) => self.text_object_mut().move_text_position(p),
            Operation::MoveTextPositionAndSetLeading(p) => {
                self.text_object_mut().set_leading(-p.y);
                self.text_object_mut().move_text_position(p);
            }
            Operation::SetTextMatrix(m) => self.text_object_mut().set_text_matrix(m),
            Operation::MoveToStartOfNextLine => {
                let leading = self.stack.last().unwrap().text_object.leading;
                self.text_object_mut()
                    .move_text_position(Point::new(0.0, -leading));
            }

            // Text Showing Operations
            Operation::ShowText(text) => self.show_text(&text.to_bytes().unwrap()),
            Operation::ShowTexts(texts) => self.show_texts(&texts),

            // Color Operations
            Operation::SetStrokeColorSpace(args) => {
                self.current_mut().stroke_color_space = args
                    .create_color_space(self.resources.resolver(), Some(self.resources))
                    .unwrap()
            }
            Operation::SetFillColorSpace(args) => {
                self.current_mut().fill_color_space = args
                    .create_color_space(self.resources.resolver(), Some(self.resources))
                    .unwrap()
            }
            Operation::SetStrokeColor(args) => self.current_mut().set_stroke_color_args(args),
            Operation::SetStrokeGray(color) => self
                .current_mut()
                .set_stroke_color_and_space(Box::new(color_space::DeviceGray()), &color),
            Operation::SetStrokeCMYK(color) => self
                .current_mut()
                .set_stroke_color_and_space(Box::new(color_space::DeviceCMYK()), &color),
            Operation::SetStrokeRGB(color) => self
                .current_mut()
                .set_stroke_color(color_space::DeviceRGB().to_skia_color(&color)),
            Operation::SetFillColor(args) => self.current_mut().set_fill_color_args(args),
            Operation::SetFillGray(color) => self
                .current_mut()
                .set_fill_color_and_space(Box::new(color_space::DeviceGray()), &color),
            Operation::SetFillCMYK(color) => self
                .current_mut()
                .set_fill_color_and_space(Box::new(color_space::DeviceCMYK()), &color),
            Operation::SetFillRGB(color) => self
                .current_mut()
                .set_fill_color_and_space(Box::new(color_space::DeviceRGB()), &color),
            Operation::SetFillColorOrWithPattern(name) => {
                self.set_fill_color_or_pattern(&name).unwrap()
            }

            // XObject Operation
            Operation::PaintXObject(name) => self.paint_x_object(&name),

            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }

    fn stroke(&mut self) {
        if let Some(p) = self.path.finish() {
            let state = self.stack.last().unwrap();
            let paint = state.get_stroke_paint();
            let stroke = state.get_stroke();
            debug!("stroke: {:?} {:?}", &paint, stroke);
            debug!("stroke: {:?}", p);
            self.canvas.stroke_path(
                p,
                &paint,
                stroke,
                state.user_to_device.into_skia(),
                state.get_mask(),
            );
        } else {
            debug!("stroke: empty or invalid path");
        }
        self.path.reset();
    }

    fn end_path(&mut self) {
        self.path.clear();
    }

    fn close_path(&mut self) {
        self.path.close_path();
    }

    fn close_and_stroke(&mut self) {
        self.close_path();
        self.stroke();
    }

    fn _fill(&mut self, fill_rule: FillRule, reset_path: bool) {
        let state = self.stack.last().unwrap();
        let paint = state.get_fill_paint();
        if let Some(p) = self.path.finish() {
            self.canvas.fill_path(
                p,
                &paint,
                fill_rule,
                state.user_to_device.into_skia(),
                state.get_mask(),
            );
        }
        if reset_path {
            self.path.reset();
        }
    }

    fn fill_path_non_zero(&mut self) {
        self._fill(FillRule::Winding, true);
    }

    fn fill_path_even_odd(&mut self) {
        self._fill(FillRule::EvenOdd, true);
    }

    fn fill_and_stroke_non_zero(&mut self) {
        self._fill(FillRule::Winding, false);
        self.stroke();
    }

    fn fill_and_stroke_even_odd(&mut self) {
        self._fill(FillRule::EvenOdd, false);
        self.stroke();
    }

    fn close_fill_and_stroke_non_zero(&mut self) {
        self.close_path();
        self.fill_and_stroke_non_zero();
    }

    fn close_fill_and_stroke_even_odd(&mut self) {
        self.close_path();
        self.fill_and_stroke_even_odd();
    }

    /// Paints the specified XObject. Only XObjectType::Image supported
    fn paint_x_object(&mut self, name: &NameOfDict) {
        fn load_image<'a, 'b>(
            image_dict: &XObjectDict<'a, 'b>,
            resources: &ResourceDict<'a, 'b>,
        ) -> RgbaImage {
            let image = image_dict.as_image().expect("Only Image XObject supported");
            image
                .decode_image(resources.resolver(), Some(resources))
                .unwrap()
                .into_rgba8()
        }

        fn load_image_as_mask<'a, 'b>(
            page_width: u32,
            page_height: u32,
            img_dict: &XObjectDict<'a, 'b>,
            resources: &ResourceDict<'a, 'b>,
            state: &State,
        ) -> AnyResult<Mask> {
            let paint = PixmapPaint {
                quality: FilterQuality::Nearest,
                ..Default::default()
            };

            let mut canvas = Pixmap::new(page_width, page_height).unwrap();
            let img = img_dict.as_image().unwrap();
            let img = img.decode_image(resources.resolver(), Some(resources))?;
            let mut img = img.into_rgba8();
            img.pixels_mut().for_each(|p| {
                p[3] = p[0];
            });

            let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
            canvas.draw_pixmap(
                0,
                0,
                img,
                &paint,
                state.image_transform(img.width(), img.height()).into_skia(),
                None,
            );
            Ok(Mask::from_pixmap(canvas.as_ref(), MaskType::Alpha))
        }

        let xobjects = self.resources.x_object().unwrap();
        let xobject = xobjects.get(&name.0).unwrap();

        let state = self.stack.last().unwrap();

        if xobject.image_mask().unwrap() {
            let mask = load_image_as_mask(
                self.device_width(),
                self.device_height(),
                xobject,
                self.resources,
                state,
            )
            .unwrap();
            // fill canvas with current fill paint with mask
            let paint = state.get_fill_paint();
            self.canvas.fill_rect(
                Rect::from_xywh(
                    0.0,
                    0.0,
                    self.device_width() as f32,
                    self.device_height() as f32,
                )
                .unwrap(),
                &paint,
                Transform::identity(),
                Some(&mask),
            );
            return;
        }

        let s_mask = xobject.s_mask().unwrap().map(|s_mask| {
            load_image_as_mask(
                self.device_width(),
                self.device_height(),
                &s_mask,
                self.resources,
                state,
            )
            .unwrap()
        });

        let paint = PixmapPaint {
            quality: if xobject.interpolate().unwrap() {
                FilterQuality::Bilinear
            } else {
                FilterQuality::Nearest
            },
            ..Default::default()
        };
        let img = load_image(xobject, self.resources);
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        self.canvas.draw_pixmap(
            0,
            0,
            img,
            &paint,
            state.image_transform(img.width(), img.height()).into_skia(),
            s_mask.as_ref().or_else(|| state.get_mask()),
        );
    }

    fn set_fill_color_or_pattern(&mut self, color_or_name: &ColorArgsOrName) -> AnyResult<()> {
        match color_or_name {
            ColorArgsOrName::Name(name) => {
                let pattern = self.resources.pattern()?;
                let pattern = pattern.get(name.as_str()).unwrap();
                match pattern.pattern_type()? {
                    PatternType::Tiling => self.set_tiling_pattern(pattern.tiling_pattern()?),
                    PatternType::Shading => self.set_shading_pattern(pattern.shading_pattern()?),
                }
            }
            ColorArgsOrName::Color(args) => {
                let color = self
                    .stack
                    .last()
                    .unwrap()
                    .fill_color_space
                    .to_skia_color(args.as_ref());
                self.current_mut().set_fill_color(color);
                Ok(())
            }
        }
    }

    fn set_shading_pattern(&mut self, pattern: ShadingPatternDict) -> AnyResult<()> {
        assert_eq!(
            pattern.matrix()?,
            UserToDeviceIndependentSpace::default(),
            "matrix not supported"
        );
        assert!(pattern.ext_g_state()?.is_empty(), "ExtGState not supported");

        let shading = pattern.shading()?;
        assert!(shading.b_box()?.is_none(), "TODO: support BBox of shading");
        assert!(
            shading.background()?.is_none(),
            "TODO: support Background of shading, paint background before shading"
        );

        assert_eq!(shading.shading_type()?, ShadingType::Axial);
        let axial = shading.axial()?;
        assert_eq!(
            axial.extend()?,
            AxialExtend::new(true, true),
            "Extend not supported"
        );
        let shader = build_linear_gradient(&axial)?;
        self.stack.last_mut().unwrap().fill_paint = PaintCreator::Gradient(Paint {
            shader,
            ..Default::default()
        });
        Ok(())
    }

    fn set_tiling_pattern(&mut self, tile: TilingPatternDict<'a, 'b>) -> AnyResult<()>
    where
        'a: 'b,
    {
        assert_eq!(
            tile.paint_type()?,
            TilingPaintType::Uncolored,
            "Colored tiling pattern not supported"
        );

        let stream: &Object<'a> = tile.resolver().resolve(tile.id().unwrap())?;
        let stream = stream.as_stream()?;
        let bytes = stream.decode(tile.resolver())?;
        let (_, ops) = terminated(parse_operations, eof)(bytes.as_ref()).unwrap();
        let bbox = tile.b_box()?;
        assert_eq!(bbox.width(), tile.x_step()?, "x_step not supported");
        assert_eq!(bbox.height(), tile.y_step()?, "y_step not supported");

        let resources = tile.resources()?;
        let mut render = Render::new(
            RenderOptionBuilder::default()
                .width(bbox.width() as u32)
                .height(bbox.height() as u32)
                .build(),
            &resources,
        );
        ops.into_iter().for_each(|op| render.exec(op));
        self.stack.last_mut().unwrap().fill_paint =
            PaintCreator::Tile((render.into(), tile.matrix()?));
        Ok(())
    }

    fn gen_glyph_path(glyph_render: &mut dyn GlyphRender, gid: u16) -> PathBuilder {
        let mut path = PathBuilder::new();
        let mut sink = PathSink(&mut path);
        glyph_render.render(gid, &mut sink).unwrap();
        path
    }

    fn render_glyph(
        canvas: &mut Pixmap,
        text_clip_path: &mut Path,
        state: &State,
        path: SkiaPath,
        render_mode: TextRenderingMode,
        trans: Transform,
    ) {
        match render_mode {
            TextRenderingMode::Fill => {
                canvas.fill_path(
                    &path,
                    &state.get_fill_paint(),
                    FillRule::Winding,
                    trans,
                    state.get_mask(),
                );
            }
            TextRenderingMode::Stroke => {
                let paint = state.get_stroke_paint();
                let stroke = state.get_stroke();
                debug!("text stroke: {:?} {:?}", &paint, stroke);
                debug!("text stroke path: {:?}", &path);
                canvas.stroke_path(
                    &path,
                    &state.get_stroke_paint(),
                    state.get_stroke(),
                    trans,
                    state.get_mask(),
                );
            }
            TextRenderingMode::FillAndStroke => {
                canvas.fill_path(
                    &path,
                    &state.get_fill_paint(),
                    FillRule::Winding,
                    trans,
                    state.get_mask(),
                );
                canvas.stroke_path(
                    &path,
                    &state.get_stroke_paint(),
                    state.get_stroke(),
                    trans,
                    state.get_mask(),
                );
            }
            TextRenderingMode::Clip => {
                let path = path.transform(trans).unwrap();
                text_clip_path.path_builder().push_path(&path);
            }
            _ => {
                todo!("Unsupported text rendering mode: {:?}", render_mode);
            }
        }
    }

    fn show_text(&mut self, text: &[u8]) {
        let text_object = self.text_object();
        let char_spacing = text_object.char_spacing;
        let word_spacing = text_object.word_spacing;
        let font = self
            .font_cache
            .get_font(text_object.font_name.as_ref().unwrap())
            .unwrap();
        debug!(
            "font: {}, type: {:?}",
            text_object.font_name.as_ref().unwrap(),
            font.font_type()
        );
        let op = font.create_op().unwrap();
        let state = self.stack.last().unwrap();

        let text_object = &state.text_object;
        let font_size = text_object.font_size;
        let mut glyph_render = font.create_glyph_render(font_size).unwrap();

        let mut text_to_user_space: TextToUserSpace = text_object.matrix;
        let render_mode = text_object.render_mode;
        let mut text_clip_path = Path::default();
        let flip_y = to_device_space(state.height, state.zoom, state.ctm).into_skia();
        for ch in op.decode_chars(text) {
            let width = op.char_width(ch) as f32 / op.units_per_em() as f32 * font_size
                + char_spacing
                + if ch == 32 { word_spacing } else { 0.0 };

            let gid = op.char_to_gid(ch);
            let path = Self::gen_glyph_path(glyph_render.as_mut(), gid);
            if !path.is_empty() {
                let path = path.finish().unwrap();
                // pre transform path to user space, render_glyph() will zoom line_width,
                // pdf line_width state is in user space, but skia line_width is in device space
                // so we need to transform path to user space, and zoom line_width in device space
                let path = path.transform(text_to_user_space.into_skia()).unwrap();

                Self::render_glyph(
                    &mut self.canvas,
                    &mut text_clip_path,
                    state,
                    path,
                    render_mode,
                    flip_y,
                );
            }
            text_to_user_space = move_text_space_right(text_to_user_space, width);
        }
        drop(op);
        drop(glyph_render);
        self.text_object_mut().matrix = text_to_user_space;
        if let Some(text_clip_path) = text_clip_path.finish() {
            self.text_object_mut()
                .text_clipping_path
                .path_builder()
                .push_path(text_clip_path);
        }
    }

    fn show_texts(&mut self, texts: &[TextStringOrNumber]) {
        for t in texts {
            match t {
                TextStringOrNumber::TextString(s) => self.show_text(&s.to_bytes().unwrap()),
                TextStringOrNumber::Number(n) => {
                    self.text_object_mut().move_right(*n);
                }
            }
        }
    }

    fn end_text(&mut self) {
        self.current_mut().end_text_object();
    }
}

fn build_linear_gradient_stops(domain: Domain, f: FunctionDict) -> AnyResult<Vec<GradientStop>> {
    fn create_stop<F: Function>(f: &F, x: f32) -> AnyResult<GradientStop> {
        let rv = f.call(&[x])?;
        // TODO: use current color space to check array length, and convert to skia color
        let color = match rv.len() {
            1 => color_space::DeviceGray().to_skia_color(&rv),
            3 => color_space::DeviceRGB().to_skia_color(&rv),
            4 => color_space::DeviceCMYK().to_skia_color(&rv),
            _ => unreachable!(),
        };
        Ok(GradientStop::new(x, color))
    }

    match f.function_type()? {
        FunctionType::ExponentialInterpolation => {
            let ef = f.exponential_interpolation()?;
            assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            Ok(vec![
                create_stop(&ef, domain.start)?,
                create_stop(&ef, domain.end)?,
            ])
        }
        FunctionType::Stitching => {
            let sf = f.stitch()?;
            let mut stops = Vec::with_capacity(sf.functions()?.len() + 1);
            stops.push(create_stop(&sf, domain.start)?);
            let functions = sf.functions()?;
            for f in &functions {
                let ef = f.exponential_interpolation()?; // only support exponential interpolation
                assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            }
            for t in sf.bounds()?.iter() {
                stops.push(create_stop(&sf, *t)?);
            }
            stops.push(create_stop(&f, domain.end)?);
            Ok(stops)
        }
        _ => {
            todo!("Unsupported function type: {:?}", f.function_type()?);
        }
    }
}

fn build_linear_gradient(shading: &AxialShadingDict) -> AnyResult<tiny_skia::Shader<'static>> {
    let coords = shading.coords()?;
    let start = coords.left_lower();
    let end = coords.right_upper();
    let stops = build_linear_gradient_stops(shading.domain()?, shading.function()?)?;
    Ok(tiny_skia::LinearGradient::new(
        start.into(),
        end.into(),
        stops,
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap())
}

/// FontWidth used in Type1 and TrueType fonts
struct FirstLastFontWidth {
    range: RangeInclusive<u32>,
    widths: Vec<u32>,
    default_width: u32,
}

impl FirstLastFontWidth {
    fn _new(first_char: u32, last_char: u32, default_width: u32, widths: Vec<u32>) -> Self {
        let range = first_char..=last_char;

        Self {
            range,
            widths,
            default_width,
        }
    }

    pub fn from_type1_type(font: &Type1FontDict) -> AnyResult<Option<Self>> {
        let widths = font.widths()?;
        let first_char = font.first_char()?;
        let last_char = font.last_char()?;
        if first_char.is_none() || last_char.is_none() {
            return Ok(None);
        }

        let desc = font
            .font_descriptor()?
            .expect("missing font descriptor, if widths exist, descriptor must also exist");
        let default_width = desc.missing_width()?;

        Ok(Some(Self::_new(
            first_char.unwrap(),
            last_char.unwrap(),
            default_width,
            widths,
        )))
    }

    fn char_width(&self, ch: u32) -> u32 {
        if self.range.contains(&ch) {
            let idx = (ch - self.range.start()) as usize;
            self.widths[idx]
        } else {
            self.default_width
        }
    }
}

struct FreeTypeFontWidth<'a> {
    font: &'a FontKitFont,
}

impl<'a> FreeTypeFontWidth<'a> {
    fn new(font: &'a FontKitFont) -> Self {
        Self { font }
    }

    pub fn glyph_width(&self, gid: u32) -> u32 {
        self.font.advance(gid).unwrap().x() as u32
    }
}

struct PathSink<'a>(pub &'a mut PathBuilder);

struct FreeTypePathSink<'a> {
    path: &'a mut PathBuilder,
    scale: f32,
}

impl<'a> FreeTypePathSink<'a> {
    fn new(path: &'a mut PathBuilder, font_size: f32) -> Self {
        Self {
            path,
            scale: font_size / 1000.0,
        }
    }
}

impl<'a> font_kit::outline::OutlineSink for FreeTypePathSink<'a> {
    fn move_to(&mut self, to: Vector2F) {
        self.path.move_to(to.x() * self.scale, to.y() * self.scale);
    }

    fn line_to(&mut self, to: Vector2F) {
        self.path.line_to(to.x() * self.scale, to.y() * self.scale);
    }

    fn quadratic_curve_to(&mut self, ctrl: Vector2F, to: Vector2F) {
        self.path.quad_to(
            ctrl.x() * self.scale,
            ctrl.y() * self.scale,
            to.x() * self.scale,
            to.y() * self.scale,
        );
    }

    fn cubic_curve_to(&mut self, ctrl: LineSegment2F, to: Vector2F) {
        self.path.cubic_to(
            ctrl.from().x() * self.scale,
            ctrl.from().y() * self.scale,
            ctrl.to().x() * self.scale,
            ctrl.to().y() * self.scale,
            to.x() * self.scale,
            to.y() * self.scale,
        );
    }

    fn close(&mut self) {
        self.path.close();
    }
}

struct Type1GlyphRender<'a> {
    font: &'a FontKitFont,
    font_size: f32,
}

impl<'a> GlyphRender for Type1GlyphRender<'a> {
    fn render(&mut self, gid: u16, sink: &mut PathSink) -> AnyResult<()> {
        let mut sink = FreeTypePathSink::new(sink.0, self.font_size);
        Ok(self.font.outline(
            gid as u32,
            font_kit::hinting::HintingOptions::None,
            &mut sink,
        )?)
    }
}

trait GlyphRender {
    fn render(&mut self, gid: u16, sink: &mut PathSink) -> AnyResult<()>;
}

trait Font {
    fn font_type(&self) -> FontType;
    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>>;
    fn create_glyph_render(&self, font_size: f32) -> AnyResult<Box<dyn GlyphRender + '_>>;
}

struct Type1FontOp<'a> {
    font_width: Either<FirstLastFontWidth, FreeTypeFontWidth<'a>>,
    font: &'a FontKitFont,
    encoding: Encoding<'a>,
}

impl<'c> Type1FontOp<'c> {
    fn new<'a: 'c, 'b: 'c>(
        font_dict: Type1FontDict<'a, 'b>,
        font: &'c FontKitFont,
        is_cff: bool,
        font_data: &'c [u8],
    ) -> AnyResult<Self> {
        let font_name = font_dict.font_name()?;
        let resolve_by_name = |encoding_name: Option<&str>| -> AnyResult<Encoding> {
            if let Some(encoding_name) = encoding_name {
                return Encoding::predefined(encoding_name)
                    .ok_or_else(|| anyhow!("Unknown encoding: {}", encoding_name));
            }

            if is_cff {
                info!("scan encoding from cff font. ({})", font_name);
                let cff_file: CffFile<'c> = CffFile::open(font_data)?;
                let font: CffFont<'c> = cff_file.iter()?.next().expect("no font in cff?");
                return Ok(Encoding::new(font.encodings()?));
            }
            info!("TODO: resolve encoding from type1 font. ({})", font_name);

            // if font not embed encoding, use known encoding for the two standard symbol fonts
            match font_name.to_ascii_lowercase().as_str() {
                "symbol" => {
                    return Ok(Encoding::SYMBOL);
                }
                "zapfdingbats" => {
                    return Ok(Encoding::ZAPFDINGBATS);
                }
                _ => (),
            }

            if let Some(desc) = font_dict.font_descriptor()? {
                if desc.flags()?.contains(FontDescriptorFlags::SYMBOLIC) {
                    panic!("Symbolic font must have encoding, but not found in font file");
                }
            }

            Ok(Encoding::STANDARD)
        };

        let font_width = FirstLastFontWidth::from_type1_type(&font_dict)?
            .map_or_else(|| Either::Right(FreeTypeFontWidth::new(font)), Either::Left);
        let encoding = font_dict.encoding()?;
        let encoding = match encoding {
            Some(NameOrDictByRef::Dict(d)) => {
                let encoding_dict = EncodingDict::new(None, d, font_dict.resolver())?;
                let r = resolve_by_name(encoding_dict.base_encoding()?)?;
                if let Some(diff) = encoding_dict.differences()? {
                    r.apply_differences(&diff)
                } else {
                    r
                }
            }
            Some(NameOrDictByRef::Name(name)) => resolve_by_name(Some(name.as_ref()))?,
            None => resolve_by_name(None)?,
        };
        Ok(Self {
            font_width,
            font,
            encoding,
        })
    }
}

impl<'a> FontOp for Type1FontOp<'a> {
    fn decode_chars<'b>(&'b self, text: &'b [u8]) -> Vec<u32> {
        text.iter().map(|v| *v as u32).collect()
    }

    /// Use font.glyph_for_char() if encoding is None or encoding.replace() returns None
    fn char_to_gid(&self, ch: u32) -> u16 {
        let gid_name = self.encoding.decode(ch as u8);
        if let Some(r) = self.font.glyph_by_name(gid_name) {
            r as u16
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            // .notdef gid is always be 0 for type1 font
            0
        }
    }

    fn char_width(&self, gid: u32) -> u32 {
        self.font_width.as_ref().either(
            |x| x.char_width(gid),
            |x| x.glyph_width(self.char_to_gid(gid) as u32),
        )
    }
}

/// Font implementation using freetype/(font-kit), to handle Type1 fonts
struct Type1Font<'a, 'b> {
    font_data: Vec<u8>,
    is_cff: bool,
    font: FontKitFont,
    font_dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Type1Font<'a, 'b> {
    fn new(is_cff: bool, data: Vec<u8>, font_dict: FontDict<'a, 'b>) -> AnyResult<Self> {
        debug_assert_eq!(data.capacity(), data.len());

        let font = FontKitFont::from_bytes(data.clone().into(), 0)?;
        Ok(Self {
            font_data: data,
            is_cff,
            font,
            font_dict,
        })
    }
}

impl<'a, 'b> Font for Type1Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type1
    }

    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new(
            self.font_dict.type1()?,
            &self.font,
            self.is_cff,
            self.font_data.as_slice(),
        )?))
    }

    fn create_glyph_render(&self, font_size: f32) -> AnyResult<Box<dyn GlyphRender + '_>> {
        Ok(Box::new(Type1GlyphRender {
            font: &self.font,
            font_size,
        }))
    }
}

struct TTFParserFontOp<'a> {
    face: TTFFace<'a>,
    units_per_em: u16,
}

impl<'a> FontOp for TTFParserFontOp<'a> {
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        s.iter().map(|v| *v as u32).collect()
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        self.face
            .glyph_index(unsafe { char::from_u32_unchecked(ch) })
            .unwrap_or_else(|| {
                error!("Failed convert char {} to gid", ch);
                GlyphId(ch as u16)
            })
            .0
    }

    fn char_width(&self, ch: u32) -> u32 {
        self.face
            .glyph_hor_advance(GlyphId(self.char_to_gid(ch)))
            .unwrap() as u32
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
}

struct TTFParserPathSink<'a> {
    path: &'a mut PathBuilder,
    scale: f32,
}

impl<'a> TTFParserPathSink<'a> {
    pub fn new(path: &'a mut PathBuilder, font_size: f32, units_per_em: f32) -> Self {
        Self {
            path,
            scale: font_size / units_per_em,
        }
    }
}

impl<'a> OutlineBuilder for TTFParserPathSink<'a> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x * self.scale, y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x * self.scale, y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(
            x1 * self.scale,
            y1 * self.scale,
            x * self.scale,
            y * self.scale,
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.cubic_to(
            x1 * self.scale,
            y1 * self.scale,
            x2 * self.scale,
            y2 * self.scale,
            x * self.scale,
            y * self.scale,
        );
    }

    fn close(&mut self) {
        self.path.close()
    }
}

struct TTFParserGlyphRender<'a> {
    face: TTFFace<'a>,
    font_size: f32,
    units_per_em: f32,
}

impl<'a> GlyphRender for TTFParserGlyphRender<'a> {
    fn render(&mut self, gid: u16, sink: &mut PathSink) -> AnyResult<()> {
        let mut sink = TTFParserPathSink::new(sink.0, self.font_size, self.units_per_em);
        self.face.outline_glyph(GlyphId(gid), &mut sink);
        Ok(())
    }
}

struct TTFParserFont<'a, 'b> {
    typ: FontType,
    data: Vec<u8>,
    font_dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Font for TTFParserFont<'a, 'b> {
    fn font_type(&self) -> FontType {
        self.typ
    }

    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(match self.font_type() {
            FontType::TrueType => {
                let face = TTFFace::parse(&self.data, 0)?;
                Box::new(TTFParserFontOp {
                    units_per_em: face.units_per_em(),
                    face,
                })
            }
            FontType::Type0 => Box::new(Type0FontOp::new(&self.font_dict.type0()?)?),
            _ => unreachable!(
                "TTFParserFont not support font type: {:?}",
                self.font_type()
            ),
        })
    }

    fn create_glyph_render(&self, font_size: f32) -> AnyResult<Box<dyn GlyphRender + '_>> {
        let face = TTFFace::parse(&self.data, 0)?;
        Ok(Box::new(TTFParserGlyphRender {
            units_per_em: face.units_per_em() as f32,
            face,
            font_size,
        }))
    }
}

static SYSTEM_FONTS: Lazy<Database> = Lazy::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
    db
});

fn standard_14_type1_font_data(font_name: &str) -> Option<&'static [u8]> {
    match font_name {
        "courier" => Some(&include_bytes!("../../../fonts/n022003l.pfb")[..]),
        "courier-bold" => Some(&include_bytes!("../../../fonts/n022004l.pfb")[..]),
        "courier-boldoblique" => Some(&include_bytes!("../../../fonts/n022024l.pfb")[..]),
        "courier-oblique" => Some(&include_bytes!("../../../fonts/n022023l.pfb")[..]),
        "helvetica" => Some(&include_bytes!("../../../fonts/n019003l.pfb")[..]),
        "helvetica-bold" => Some(&include_bytes!("../../../fonts/n019004l.pfb")[..]),
        "helvetica-boldoblique" => Some(&include_bytes!("../../../fonts/n019024l.pfb")[..]),
        "helvetica-oblique" => Some(&include_bytes!("../../../fonts/n019023l.pfb")[..]),
        "symbol" => Some(&include_bytes!("../../../fonts/s050000l.pfb")[..]),
        "times-bold" => Some(&include_bytes!("../../../fonts/n021004l.pfb")[..]),
        "times-bolditalic" => Some(&include_bytes!("../../../fonts/n021024l.pfb")[..]),
        "times-italic" => Some(&include_bytes!("../../../fonts/n021023l.pfb")[..]),
        "times-roman" => Some(&include_bytes!("../../../fonts/n021003l.pfb")[..]),
        "zapfdingbats" => Some(&include_bytes!("../../../fonts/d050000l.pfb")[..]),
        _ => None,
    }
}

struct FontCache<'c> {
    fonts: HashMap<String, Box<dyn Font + 'c>>,
}

impl<'c> FontCache<'c> {
    fn load_true_type_font_from_bytes<'a, 'b>(
        font: FontDict<'a, 'b>,
        bytes: Vec<u8>,
    ) -> AnyResult<TTFParserFont<'a, 'b>> {
        Ok(TTFParserFont {
            typ: font.subtype()?,
            data: bytes,
            font_dict: font,
        })
    }

    fn load_true_type_from_os(desc: &FontDescriptorDict) -> AnyResult<Vec<u8>> {
        let font_name = desc.font_name()?;
        let mut families = vec![Family::Name(font_name)];
        let family = desc.font_family()?;
        if let Some(family) = &family {
            if !family.is_empty() {
                families.push(Family::Name(family));
            }
        }
        let flags = desc.flags()?;
        if flags & FontDescriptorFlags::SERIF == FontDescriptorFlags::SERIF {
            families.push(Family::Serif);
        }
        if flags & FontDescriptorFlags::FIXED_PITCH == FontDescriptorFlags::FIXED_PITCH {
            families.push(Family::Monospace);
        }
        let style = if flags & FontDescriptorFlags::ITALIC == FontDescriptorFlags::ITALIC {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };

        let mut q = Query {
            families: &families,
            weight: desc
                .font_weight()?
                .map(|v| Weight(v as u16))
                .unwrap_or(Weight::NORMAL),
            style,
            ..Default::default()
        };
        if let Some(stretch) = desc.font_stretch()? {
            q.stretch = stretch.into();
        }

        let id = SYSTEM_FONTS.query(&q).expect("font not found in system");
        let face = SYSTEM_FONTS.face(id).unwrap();
        assert_eq!(face.index, 0, "Only one face supported");
        match face.source {
            Source::File(ref path) => {
                let mut file = File::open(path)?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            Source::Binary(ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
            Source::SharedFile(_, ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
        }
    }

    fn load_embed_font_bytes<'a>(
        resolver: &ObjectResolver<'a>,
        s: &Stream<'a>,
    ) -> AnyResult<Vec<u8>> {
        Ok(s.decode(resolver)?.into_owned())
    }

    fn load_ttf_parser_font<'a, 'b>(
        font: FontDict<'a, 'b>,
        resolve_desc: fn(&FontDict<'a, 'b>) -> AnyResult<FontDescriptorDict<'a, 'b>>,
    ) -> AnyResult<TTFParserFont<'a, 'b>> {
        let desc = resolve_desc(&font)?;
        let bytes = match desc.font_file2()? {
            Some(stream) => Self::load_embed_font_bytes(desc.resolver(), stream)?,
            None => {
                let font_name = desc.font_name()?;
                warn!(
                    "font {} not found in file, try to load from system",
                    font_name,
                );
                Self::load_true_type_from_os(&desc)?
            }
        };
        Self::load_true_type_font_from_bytes(font, bytes)
    }

    /// Load Type1 font, only standard 14 fonts supported, these fonts are replaced
    /// by TrueType fonts scanned from current OS. Because Type1 fonts are not
    /// supported by swash, and the only crate support Type1 fonts is `font`, which
    /// I am not familiar with.
    fn load_type1_font<'a, 'b>(font: FontDict<'a, 'b>) -> AnyResult<Type1Font<'a, 'b>>
    where
        'a: 'c,
        'b: 'c,
    {
        let f = font.type1()?;
        let font_name = f.font_name()?.to_lowercase();
        let desc = f.font_descriptor()?;
        let font_data = desc
            .map(|desc| -> AnyResult<_> {
                let r = desc
                    .font_file()
                    .map(|s| s.map(|s| (false, s)))
                    .transpose()
                    .or_else(
                        || desc.font_file3().map(|s| s.map(|s| (true, s))).transpose(), /* if Compact Font Format*/
                    )
                    .transpose();
                r
            })
            .transpose()?
            .flatten();
        let (is_cff, mut bytes) = match font_data {
            Some(s) => (s.0, Self::load_embed_font_bytes(f.resolver(), s.1)?),
            None => (
                false,
                standard_14_type1_font_data(font_name.as_str())
                    .expect("Failed to find font data")
                    .to_owned(),
            ),
        };
        bytes.shrink_to_fit();
        Type1Font::new(is_cff, bytes, font)
    }

    fn scan_font<'a, 'b>(font: FontDict<'a, 'b>) -> AnyResult<Option<Box<dyn Font + 'c>>>
    where
        'a: 'c,
        'b: 'c,
    {
        match font.subtype()? {
            FontType::TrueType => Ok(Some(Box::new(Self::load_ttf_parser_font(font, |f| {
                let tt = f.truetype()?;
                Ok(tt.font_descriptor()?.unwrap())
            })?))),

            FontType::Type0 => Ok(Some(Box::new(Self::load_ttf_parser_font(font, |f| {
                let type0_font = f.type0()?;
                let descentdant_fonts = type0_font.descendant_fonts()?;
                assert_eq!(
                    descentdant_fonts.len(),
                    1,
                    "Type0 font should have one descendant fonts"
                );
                let descentdant_font = descentdant_fonts.into_iter().next().unwrap();
                assert_eq!(
                    descentdant_font.subtype()?,
                    CIDFontType::CIDFontType2,
                    "Only CIDFontType2 supported"
                );
                Ok(descentdant_font.font_descriptor()?.unwrap())
            })?))),

            FontType::Type1 => {
                Self::load_type1_font(font).map(|v| Some(Box::new(v) as Box<dyn Font + 'c>))
            }
            _ => {
                error!("Unsupported font type: {:?}", font.subtype()?);
                Ok(None)
            }
        }
    }

    fn new<'a, 'b>(resource: &'c ResourceDict<'a, 'b>) -> anyhow::Result<Self>
    where
        'a: 'c,
        'b: 'c,
    {
        let font_res = resource.font()?;
        let mut fonts = HashMap::with_capacity(font_res.len());
        for (k, v) in font_res.into_iter() {
            info!("load font: {:?}", k);
            let font = Self::scan_font(v)?;
            if let Some(font) = font {
                fonts.insert(k, font);
            }
        }
        Ok(Self { fonts })
    }

    fn get_font(&self, s: &str) -> Option<&dyn Font> {
        self.fonts.get(s).map(|x| x.as_ref())
    }
}

trait FontOp {
    /// Decode char codes to chars, possible using some encoding
    fn decode_chars(&self, s: &[u8]) -> Vec<u32>;
    fn char_to_gid(&self, ch: u32) -> u16;
    /// Return glyph width for specified char
    fn char_width(&self, ch: u32) -> u32;
    fn units_per_em(&self) -> u16 {
        1000
    }
}

struct Type0FontOp {
    widths: CIDFontWidths,
    default_width: u32,
}

impl Type0FontOp {
    fn new(font: &Type0FontDict) -> AnyResult<Self> {
        if let NameOrStream::Name(ref encoding) = font.encoding()? {
            assert_eq!(encoding.as_ref(), "Identity-H");
            // assert_eq!(encoding.as_ref(), CIDFontEncoding::IdentityH.as_ref());
        } else {
            todo!("Only IdentityH encoding supported");
        }
        let cid_fonts = font.descendant_fonts()?;
        let cid_font = cid_fonts.get(0).unwrap();
        let widths = cid_font.w()?;
        Ok(Self {
            widths,
            default_width: cid_font.dw()?,
        })
    }
}

impl FontOp for Type0FontOp {
    /// `s` each two bytes as a char code, big endian. append 0 if len(s) is odd
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        debug_assert!(s.len() % 2 == 0, "{:?}", s);
        let mut rv = Vec::with_capacity(s.len() / 2);
        for i in 0..s.len() / 2 {
            let ch = u16::from_be_bytes([s[i * 2], s[i * 2 + 1]]);
            rv.push(ch as u32);
        }
        rv
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        ch as u16
    }

    fn char_width(&self, ch: u32) -> u32 {
        self.widths.char_width(ch).unwrap_or(self.default_width)
    }
}

#[derive(Educe, Clone)]
#[educe(Debug)]
struct TextObject {
    matrix: TextToUserSpace,
    line_matrix: TextToUserSpace,
    font_size: f32,
    font_name: Option<String>,
    text_clipping_path: Path,

    char_spacing: f32,              // Tc
    word_spacing: f32,              // Tw
    horiz_scaling: f32,             // Th
    leading: f32,                   // Tl
    render_mode: TextRenderingMode, // Tmode
    rise: f32,                      // Trise
    knockout: bool,                 // Tk
}

impl TextObject {
    pub fn new() -> Self {
        Self {
            matrix: TextToUserSpace::identity(),
            line_matrix: TextToUserSpace::identity(),
            font_size: 0.0,
            font_name: None,
            text_clipping_path: Path::default(),

            char_spacing: 0.0,
            word_spacing: 0.0,
            horiz_scaling: 100.0,
            leading: 0.0,
            render_mode: TextRenderingMode::Fill,
            rise: 0.0,
            knockout: true,
        }
    }

    fn reset(&mut self) {
        self.matrix = TextToUserSpace::identity();
        self.line_matrix = TextToUserSpace::identity();
    }

    fn set_font(&mut self, name: &NameOfDict, size: f32) {
        self.font_size = size;
        self.font_name = Some(name.0.to_owned());
    }

    fn move_text_position(&mut self, p: Point) {
        let matrix = move_text_space_pos(self.line_matrix, p.x, p.y);
        self.matrix = matrix;
        self.line_matrix = matrix;
    }

    fn set_text_matrix(&mut self, m: TextToUserSpace) {
        self.matrix = m;
        self.line_matrix = m;
    }

    fn move_right(&mut self, n: f32) {
        let tx = -n * 0.001 * self.font_size;
        self.matrix = move_text_space_right(self.matrix, tx);
    }

    fn set_character_spacing(&mut self, spacing: f32) {
        self.char_spacing = spacing;
    }

    fn set_word_spacing(&mut self, spacing: f32) {
        self.word_spacing = spacing;
    }

    fn set_horizontal_scaling(&mut self, scale: f32) {
        self.horiz_scaling = scale;
    }

    fn set_leading(&mut self, leading: f32) {
        self.leading = leading;
    }

    fn set_text_rendering_mode(&mut self, mode: TextRenderingMode) {
        self.render_mode = mode;
    }

    fn set_text_rise(&mut self, rise: f32) {
        self.rise = rise;
    }
}

#[cfg(test)]
mod tests;
