use std::{borrow::Cow, collections::HashMap, ops::RangeInclusive};

use crate::{
    file::XObjectDict,
    function::{Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        parse_operations, AxialExtend, AxialShadingDict, Color, ColorOrName, ColorSpace,
        ConvertFromObject, LineCapStyle, LineJoinStyle, NameOfDict, PatternType, Point,
        RenderingIntent, ShadingPatternDict, ShadingType, TextRenderingMode, TilingPaintType,
        TilingPatternDict, TransformMatrix,
    },
    object::{Array, FilterDecodedData, Object, PdfObject, TextStringOrNumber},
    text::{FontDict, FontType},
};
use anyhow::{anyhow, Result as AnyResult};
use educe::Educe;
use image::RgbaImage;
use itertools::Either;
use log::{debug, error, info};
use nom::{combinator::eof, sequence::terminated};
use swash::{
    scale::ScaleContext,
    zeno::{Command as PathCommand, PathData},
    CacheKey, FontRef,
};

use super::{GraphicsStateParameterDict, Operation, Rectangle, ResourceDict};
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, Mask, MaskType, Paint, Path as SkiaPath, PathBuilder,
    Pixmap, PixmapPaint, PixmapRef, Point as SkiaPoint, Stroke, StrokeDash, Transform,
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

impl From<Color> for tiny_skia::Color {
    fn from(color: Color) -> Self {
        match color {
            Color::Rgb(r, g, b) => tiny_skia::Color::from_rgba(r, g, b, 1.0).unwrap(),
            Color::Cmyk(c, m, y, k) => tiny_skia::Color::from_rgba(
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
                1.0,
            )
            .unwrap(),
            Color::Gray(g) => tiny_skia::Color::from_rgba(g, g, g, 1.0).unwrap(),
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
    Tile((Pixmap, TransformMatrix)),
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
                let width = p.width() as f32;
                let height = p.height() as f32;
                let matrix_mapper = MatrixMapper::new(width, height, 1.0, *matrix);
                r.shader = tiny_skia::Pattern::new(
                    p.as_ref(),
                    tiny_skia::SpreadMode::Repeat,
                    FilterQuality::Bicubic,
                    1.0f32,
                    matrix_mapper.tile_transform(),
                );
                Cow::Owned(r)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct State {
    ctm: MatrixMapper,
    fill_paint: PaintCreator,
    stroke_paint: PaintCreator,
    stroke: Stroke,
    mask: Option<Mask>,
    fill_color_space: ColorSpace,
    stroke_color_space: ColorSpace,
    text_block: TextBlock,
}

impl State {
    /// height: height in user space coordinate
    fn new(option: &RenderOption) -> Self {
        let mut r = Self {
            ctm: MatrixMapper::new(
                option.width as f32 * option.zoom,
                option.height as f32 * option.zoom,
                option.zoom,
                TransformMatrix::identity(),
            ),
            fill_paint: PaintCreator::Color(tiny_skia::Color::TRANSPARENT),
            stroke_paint: PaintCreator::Color(tiny_skia::Color::BLACK),
            stroke: Stroke::default(),
            mask: None,
            fill_color_space: ColorSpace::DeviceRGB,
            stroke_color_space: ColorSpace::DeviceRGB,
            text_block: TextBlock::new(),
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
        log::info!("not implemented: flatness: {}", flatness);
    }

    fn set_render_intent(&mut self, intent: RenderingIntent) {
        log::info!("not implemented: render intent: {}", intent);
    }

    fn set_stroke_color(&mut self, color: Color) {
        log::debug!("set stroke color: {:?}", color);
        self.stroke_paint = PaintCreator::Color(color.into());
    }

    fn set_fill_color(&mut self, color: Color) {
        log::debug!("set fill color: {:?}", color);
        self.fill_paint = PaintCreator::Color(color.into());
    }

    fn set_ctm(&mut self, ctm: TransformMatrix) {
        log::debug!("set ctm: {:?}", ctm);
        self.ctm.set_ctm(ctm);
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

    fn path_transform(&self) -> Transform {
        self.ctm.path_transform()
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> Transform {
        self.ctm.image_transform(img_w, img_h)
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
                _ => log::info!("Unknown or unsupported ExtGState key: {}", key.as_ref()),
            }
        }
    }

    fn update_mask(&mut self, path: &SkiaPath, rule: FillRule) {
        let w = self.ctm.width;
        let h = self.ctm.height;
        let mut mask = self.mask.take().unwrap_or_else(|| {
            let mut r = Mask::new(w as u32, h as u32).unwrap();
            let p = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w, h).unwrap());
            r.fill_path(&p, FillRule::Winding, true, Transform::identity());
            r
        });
        mask.intersect_path(path, rule, true, self.ctm.flip_y(Transform::identity()));
        self.mask = Some(mask);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Winding fill rule.
    fn clip_non_zero(&mut self, path: &SkiaPath) {
        self.update_mask(path, FillRule::Winding);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Even-Odd fill rule.
    fn clip_even_odd(&mut self, path: &SkiaPath) {
        self.update_mask(path, FillRule::EvenOdd);
    }

    fn set_text_knockout_flag(&mut self, knockout: bool) {
        self.text_block.knockout = knockout;
    }
}

#[derive(Debug)]
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

    fn close_path(&mut self) {
        self.path_builder().close();
    }

    fn move_to(&mut self, p: Point) {
        self.path_builder().move_to(p.x, p.y);
    }

    fn line_to(&mut self, p: Point) {
        self.path_builder().line_to(p.x, p.y);
    }

    fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path_builder()
            .cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    }

    fn curve_to_cur_point_as_control(&mut self, p2: Point, p3: Point) {
        let p1 = self.path_builder().last_point().unwrap();
        self.curve_to(Point { x: p1.x, y: p2.y }, p2, p3);
    }

    fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) {
        self.curve_to(p1, p3, p3);
    }

    fn append_rect(&mut self, p: Point, w: f32, h: f32) {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path_builder().push_rect(r.into());
    }

    /// Build path and clear the path builder
    fn finish(&mut self) -> &SkiaPath {
        if let Either::Left(_) = self.path {
            let temp = Either::Left(PathBuilder::new());
            let p = std::mem::replace(&mut self.path, temp);
            self.path = p.left_and_then(|p| Either::Right(p.finish().unwrap()));
        }

        match &self.path {
            Either::Left(_) => unreachable!(),
            Either::Right(p) => p,
        }
    }

    fn reset(&mut self) {
        let temp = Either::Left(PathBuilder::new());
        let p = std::mem::replace(&mut self.path, temp);
        self.path = p.right_and_then(|p| Either::Left(p.clear()));
    }

    fn clear(&mut self) {
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

    pub fn build(self) -> RenderOption {
        self.0
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Render<'a, 'b> {
    canvas: Pixmap,
    stack: Vec<State>,
    width: u32,
    height: u32,
    path: Path,
    #[educe(Debug(ignore))]
    font_cache: FontCache,
    resources: &'b ResourceDict<'a, 'b>,
}

impl<'a, 'b> Render<'a, 'b> {
    pub fn new(option: RenderOption, resources: &'b ResourceDict<'a, 'b>) -> Self {
        let w = (option.width as f32 * option.zoom) as u32;
        let h = (option.height as f32 * option.zoom) as u32;

        let mut canvas = Pixmap::new(w, h).unwrap();
        // fill the whole canvas with white
        canvas.fill(tiny_skia::Color::WHITE);
        Self {
            canvas,
            stack: vec![State::new(&option)],
            width: w,
            height: h,
            path: Path::default(),
            font_cache: FontCache::new(resources).unwrap(),
            resources,
        }
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
        self.canvas
    }

    fn text_block(&self) -> &TextBlock {
        &self.stack.last().unwrap().text_block
    }

    fn text_block_mut(&mut self) -> &mut TextBlock {
        &mut self.current_mut().text_block
    }

    pub(crate) fn exec(&mut self, op: &Operation<'_>) {
        match op {
            // General Graphics State Operations
            Operation::SetLineWidth(width) => self.current_mut().set_line_width(*width),
            Operation::SetLineCap(cap) => self.current_mut().set_line_cap(*cap),
            Operation::SetLineJoin(join) => self.current_mut().set_line_join(*join),
            Operation::SetMiterLimit(limit) => self.current_mut().set_miter_limit(*limit),
            Operation::SetDashPattern(pattern, phase) => {
                self.current_mut().set_dash_pattern(pattern, *phase)
            }
            Operation::SetRenderIntent(intent) => self.current_mut().set_render_intent(*intent),
            Operation::SetFlatness(flatness) => self.current_mut().set_flatness(*flatness),
            Operation::SetGraphicsStateParameters(nm) => {
                let res = self.resources.ext_g_state().unwrap();
                let res = res.get(&nm.0).expect("ExtGState not found");
                self.current_mut().set_graphics_state(res);
            }

            // Special Graphics State Operations
            Operation::SaveGraphicsState => self.push(),
            Operation::RestoreGraphicsState => self.pop(),
            Operation::ModifyCTM(ctm) => self.current_mut().set_ctm(*ctm),

            // Path Construction Operations
            Operation::MoveToNext(p) => self.path.move_to(*p),
            Operation::LineToNext(p) => self.path.line_to(*p),
            Operation::AppendBezierCurve(p1, p2, p3) => self.path.curve_to(*p1, *p2, *p3),
            Operation::AppendBezierCurve2(p2, p3) => {
                self.path.curve_to_cur_point_as_control(*p2, *p3);
            }
            Operation::AppendBezierCurve1(p1, p3) => {
                self.path.curve_to_dest_point_as_control(*p1, *p3);
            }
            Operation::ClosePath => self.path.close_path(),
            Operation::AppendRectangle(p, w, h) => self.path.append_rect(*p, *w, *h),

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
                state.clip_non_zero(self.path.finish());
            }
            Operation::ClipEvenOdd => {
                let state = self.stack.last_mut().unwrap();
                state.clip_even_odd(self.path.finish());
            }

            // Text Object Operations
            Operation::BeginText => {
                self.text_block_mut().reset();
            }
            Operation::EndText => {}

            // Text State Operations
            Operation::SetCharacterSpacing(spacing) => {
                self.text_block_mut().set_character_spacing(*spacing)
            }
            Operation::SetWordSpacing(spacing) => self.text_block_mut().set_word_spacing(*spacing),
            Operation::SetHorizontalScaling(scale) => {
                self.text_block_mut().set_horizontal_scaling(*scale)
            }
            Operation::SetLeading(leading) => self.text_block_mut().set_leading(*leading),
            Operation::SetFont(name, size) => self.text_block_mut().set_font(name, *size),
            Operation::SetTextRenderingMode(mode) => {
                self.text_block_mut().set_text_rendering_mode(*mode)
            }
            Operation::SetTextRise(rise) => self.text_block_mut().set_text_rise(*rise),

            // Text Positioning Operations
            Operation::MoveTextPosition(p) => self.text_block_mut().move_text_position(*p),
            Operation::SetTextMatrix(m) => self.text_block_mut().set_text_matrix(*m),

            // Text Showing Operations
            Operation::ShowText(text) => self.show_text(text),
            Operation::ShowTexts(texts) => self.show_texts(texts),

            // Color Operations
            Operation::SetStrokeColorSpace(space) => self.current_mut().stroke_color_space = *space,
            Operation::SetFillColorSpace(space) => self.current_mut().fill_color_space = *space,
            Operation::SetStrokeColor(color)
            | Operation::SetStrokeGray(color)
            | Operation::SetStrokeCMYK(color)
            | Operation::SetStrokeRGB(color) => self.current_mut().set_stroke_color(*color),
            Operation::SetFillColor(color)
            | Operation::SetFillGray(color)
            | Operation::SetFillCMYK(color)
            | Operation::SetFillRGB(color) => self.current_mut().set_fill_color(*color),
            Operation::SetFillColorOrWithPattern(name) => {
                self.set_fill_color_or_pattern(name).unwrap()
            }

            // XObject Operation
            Operation::PaintXObject(name) => self.paint_x_object(name),

            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }

    fn stroke(&mut self) {
        let state = self.stack.last().unwrap();
        let paint = state.get_stroke_paint();
        let stroke = state.get_stroke();
        log::debug!("stroke: {:?} {:?}", paint, stroke);
        self.canvas.stroke_path(
            self.path.finish(),
            &paint,
            stroke,
            state.path_transform(),
            state.get_mask(),
        );
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

    fn _fill(&mut self, fill_rule: FillRule) {
        let state = self.stack.last().unwrap();
        let paint = state.get_fill_paint();
        self.canvas.fill_path(
            self.path.finish(),
            &paint,
            fill_rule,
            state.path_transform(),
            state.get_mask(),
        );
        self.path.reset();
    }

    fn fill_path_non_zero(&mut self) {
        self._fill(FillRule::Winding);
    }

    fn fill_path_even_odd(&mut self) {
        self._fill(FillRule::EvenOdd);
    }

    fn fill_and_stroke_non_zero(&mut self) {
        self.fill_path_non_zero();
        self.stroke();
    }

    fn fill_and_stroke_even_odd(&mut self) {
        self.fill_path_even_odd();
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
    fn paint_x_object(&mut self, name: &crate::graphics::NameOfDict) {
        fn load_image(image_dict: &XObjectDict) -> RgbaImage {
            let image = image_dict.as_image().expect("Only Image XObject supported");
            let image = image.decode(image_dict.resolver(), false).unwrap();
            let FilterDecodedData::Image(img) = image else {
                panic!("Stream should decoded to image");
            };
            img.into_rgba8()
        }

        let xobjects = self.resources.x_object().unwrap();
        let xobject = xobjects.get(&name.0).unwrap();

        let smask = xobject.s_mask().unwrap();
        let mut smask_img: RgbaImage;
        let smask = if let Some(smask) = smask {
            smask_img = load_image(&smask);
            smask_img.pixels_mut().for_each(|p| {
                p[3] = p[0];
            });
            let img =
                PixmapRef::from_bytes(smask_img.as_raw(), smask_img.width(), smask_img.height())
                    .unwrap();
            img.save_png("/tmp/mask2.png").unwrap();
            let mask = Mask::from_pixmap(img, MaskType::Alpha);
            mask.save_png("/tmp/mask.png").unwrap();
            Some(mask)
        } else {
            None
        };

        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..Default::default()
        };
        let img = load_image(xobject);
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        let transform = self
            .stack
            .last()
            .unwrap()
            .image_transform(img.width(), img.height());
        self.canvas
            .draw_pixmap(0, 0, img, &paint, transform, smask.as_ref());
    }

    fn set_fill_color_or_pattern(
        &mut self,
        color_or_name: &crate::graphics::ColorOrName,
    ) -> AnyResult<()> {
        let ColorOrName::Name(name) = color_or_name else {
            panic!("Only Name supported");
        };

        let pattern = self.resources.pattern()?;
        let pattern = pattern.get(name.as_str()).unwrap();
        match pattern.pattern_type()? {
            PatternType::Tiling => self.set_tiling_pattern(pattern.tiling_pattern()?),
            PatternType::Shading => self.set_shading_pattern(pattern.shading_pattern()?),
        }
    }

    fn set_shading_pattern(&mut self, pattern: ShadingPatternDict) -> AnyResult<()> {
        assert_eq!(
            pattern.matrix()?,
            TransformMatrix::identity(),
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

    fn set_tiling_pattern(&mut self, tile: TilingPatternDict<'a, 'b>) -> AnyResult<()> {
        assert_eq!(
            tile.paint_type()?,
            TilingPaintType::Uncolored,
            "Colored tiling pattern not supported"
        );

        let stream: &Object<'a> = tile.resolver().resolve(tile.id().unwrap())?;
        let stream = stream.as_stream()?;
        let decoded = stream.decode(tile.resolver(), false)?;
        let bytes = decoded.as_bytes();
        let (_, ops) = terminated(parse_operations, eof)(bytes).unwrap();
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
        for op in ops {
            render.exec(&op);
        }
        self.stack.last_mut().unwrap().fill_paint =
            PaintCreator::Tile((render.into(), tile.matrix()?));
        Ok(())
    }

    fn show_text(&mut self, text: &str) {
        let state = self.stack.last().unwrap();
        debug!(
            "show_text: {:?}, paint: {:?}/{:?}",
            text,
            &state.get_fill_paint(),
            &state.get_stroke_paint(),
        );

        let text_block = self.text_block();
        let font_size = text_block.font_size;
        let render_mode = text_block.render_mode;
        let font = self
            .font_cache
            .get_font(text_block.font_name.as_ref().unwrap())
            .unwrap();
        let font_ref = font.as_ref();
        let mut context = ScaleContext::new();
        let builder = context.builder(font_ref).size(font_size).hint(true);
        let builder = if let Some(weight) = font.weight {
            builder.variations(&[("wght", weight as f32)])
        } else {
            builder
        };
        let mut scaler = builder.build();
        let mut transform: Transform = text_block.matrix.into();
        let ctm = &state.ctm;
        for ch in text.chars() {
            let glyph_id = font_ref.charmap().map(ch);
            let outline = scaler.scale_outline(glyph_id).unwrap();

            let mut path = PathBuilder::new();
            (0..outline.len()).for_each(|idx| {
                let layer = outline.get(idx).unwrap();
                layer.path().commands().for_each(|v| match v {
                    PathCommand::MoveTo(p) => {
                        path.move_to(p.x, p.y);
                    }
                    PathCommand::LineTo(p) => {
                        path.line_to(p.x, p.y);
                    }
                    PathCommand::CurveTo(p1, p2, p3) => {
                        path.cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
                    }
                    PathCommand::QuadTo(p1, p2) => {
                        path.quad_to(p1.x, p1.y, p2.x, p2.y);
                    }
                    PathCommand::Close => {
                        path.close();
                    }
                })
            });
            let width = font.font_width(ch) as f32 / 1000.0 * font_size;
            debug!("width: {width}");
            if path.is_empty() {
                transform = transform.pre_translate(width, 0.0);
                continue;
            }

            let path = path.finish().unwrap();
            let ctm_transform: Transform = ctm.ctm.into();
            debug!("ctm: {:?}", ctm_transform);
            debug!("transform: {:?}", transform);
            let trans = transform;
            let trans = ctm_transform.pre_concat(trans);
            debug!("trans: {:?}", trans);
            let trans = Transform {
                sx: trans.sx * ctm.zoom,
                kx: trans.kx,
                ky: trans.ky,
                sy: trans.sy * -ctm.zoom,
                tx: trans.tx * ctm.zoom,
                ty: ctm.height * ctm.zoom - trans.ty * ctm.zoom,
            };
            debug!("trans: {:?}, height: {}", trans, ctm.height);
            match render_mode {
                TextRenderingMode::Fill => {
                    self.canvas.fill_path(
                        &path,
                        &state.get_fill_paint(),
                        FillRule::Winding,
                        trans,
                        state.get_mask(),
                    );
                }
                TextRenderingMode::FillAndStroke => {
                    self.canvas.fill_path(
                        &path,
                        &state.get_fill_paint(),
                        FillRule::Winding,
                        trans,
                        state.get_mask(),
                    );
                    self.canvas.stroke_path(
                        &path,
                        &state.get_stroke_paint(),
                        &state.get_stroke(),
                        trans,
                        state.get_mask(),
                    );
                }
                _ => {
                    todo!("Unsupported text rendering mode: {:?}", render_mode);
                }
            }
            transform = transform.pre_translate(width, 0.0);
        }
        self.text_block_mut().matrix = transform.into();
    }

    fn show_texts(&mut self, texts: &[TextStringOrNumber]) {
        for t in texts {
            match t {
                TextStringOrNumber::Text(s) => self.show_text(s),
                TextStringOrNumber::Number(n) => {
                    self.text_block_mut().move_right(*n);
                }
                TextStringOrNumber::HexText(_) => {
                    error!("HexText not supported");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct MatrixMapper {
    // height of device space coordinate
    height: f32,
    // width of device space coordinate
    width: f32,
    zoom: f32,
    ctm: TransformMatrix,
}

impl MatrixMapper {
    /// width/height: height of device space coordinate
    pub fn new(width: f32, height: f32, zoom: f32, ctm: TransformMatrix) -> Self {
        Self {
            width,
            height,
            zoom,
            ctm,
        }
    }

    pub fn set_ctm(&mut self, ctm: TransformMatrix) {
        self.ctm = ctm;
    }

    pub fn path_transform(&self) -> Transform {
        self.flip_y(self.ctm.into())
    }

    pub fn tile_transform(&self) -> Transform {
        self.flip_y(self.ctm.into())
    }

    fn flip_y(&self, t: Transform) -> Transform {
        t.pre_scale(self.zoom, -self.zoom)
            .pre_translate(0.0, -self.height)
    }

    pub fn image_transform(&self, img_w: u32, img_h: u32) -> Transform {
        Transform::from_row(
            self.ctm.sx / img_w as f32 * self.zoom,
            0.0,
            0.0,
            self.ctm.sy / img_h as f32 * self.zoom,
            self.ctm.tx * self.zoom,
            self.height - self.ctm.ty * self.zoom - self.ctm.sy * self.zoom,
        )
    }
}

fn build_linear_gradient_stops(domain: Domain, f: FunctionDict) -> AnyResult<Vec<GradientStop>> {
    fn create_stop<F: Function>(f: &F, x: f32) -> AnyResult<GradientStop> {
        let rv = f.call(&[x])?;
        let mut arr = rv.into_iter().map(Object::Number).collect::<Array>();
        // TODO: Optimize speed of convert Vec<f32> to Color instead of using Object array
        let color = Color::convert_from_object(&mut arr)?;
        Ok(GradientStop::new(x, color.into()))
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

struct FontWidth {
    range: Option<RangeInclusive<u32>>,
    widths: Vec<u32>,
    default_width: u32,
}

impl FontWidth {
    fn new(font: &FontDict) -> AnyResult<Self> {
        let (first_char, last_char, default_width, widths) = match font.subtype()? {
            FontType::TrueType => {
                let font = font.truetype()?;
                let desc = font
                    .font_descriptor()?
                    .ok_or_else(|| anyhow!("font descriptor failed to load"))?;
                let widths = font.widths()?;
                let first_char = font.first_char()?;
                let last_char = font.last_char()?;
                let default_width = desc.missing_width()?;
                (first_char, last_char, default_width, widths)
            }
            FontType::Type1 => {
                let font = font.type1()?;
                let desc = font
                    .font_descriptor()?
                    .ok_or_else(|| anyhow!("font descriptor failed to load"))?;
                let widths = font.widths()?;
                let first_char = font.first_char()?;
                let last_char = font.last_char()?;
                let default_width = desc.missing_width()?;
                (first_char, last_char, default_width, widths)
            }
            _ => anyhow::bail!("Unsupported font type: {:?}", font.subtype()?),
        };

        let range = if let (Some(first_char), Some(last_char)) = (first_char, last_char) {
            Some(first_char..=last_char)
        } else {
            None
        };

        Ok(Self {
            range,
            widths,
            default_width,
        })
    }

    fn char_width(&self, ch: char) -> u32 {
        match &self.range {
            Some(range) if range.contains(&(ch as u32)) => {
                let idx = (ch as u32 - range.start()) as usize;
                self.widths[idx]
            }
            _ => self.default_width,
        }
    }
}

struct Font {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
    font_width: FontWidth,
    weight: Option<u16>,
}

impl Font {
    fn as_ref(&self) -> FontRef {
        FontRef {
            data: &self.data[..],
            offset: self.offset,
            key: self.key,
        }
    }

    fn font_width(&self, ch: char) -> u32 {
        self.font_width.char_width(ch)
    }
}

struct FontCache {
    fonts: HashMap<String, Font>,
}

impl FontCache {
    fn scan_font(font: &FontDict) -> AnyResult<Option<Font>> {
        match font.subtype()? {
            FontType::TrueType => {
                let true_type_font = font.truetype()?;
                let desc = true_type_font.font_descriptor()?.unwrap();
                let bytes = desc.font_file2()?.unwrap();
                let bytes = bytes.decode(desc.resolver(), false)?;
                match bytes {
                    FilterDecodedData::Bytes(_) => {
                        let bytes = bytes.to_owned();
                        let font_ref = FontRef::from_index(&bytes[..], 0)
                            .ok_or_else(|| anyhow!("Failed to load font"))?;
                        let offset = font_ref.offset;
                        let key = font_ref.key;
                        Ok(Some(Font {
                            data: bytes,
                            offset,
                            key,
                            font_width: FontWidth::new(font)?,
                            weight: desc.font_weight()?.map(|v| v as u16),
                        }))
                    }
                    _ => {
                        todo!("Unsupported font file type");
                    }
                }
            }
            _ => {
                error!("Unsupported font type: {:?}", font.subtype()?);
                Ok(None)
            }
        }
    }

    fn new(resource: &ResourceDict<'_, '_>) -> anyhow::Result<Self> {
        let font_res = resource.font()?;
        let mut fonts = HashMap::with_capacity(font_res.len());
        for (k, v) in font_res.into_iter() {
            info!("load font: {:?}", k);
            let font = Self::scan_font(&v)?;
            if let Some(font) = font {
                fonts.insert(k, font);
            }
        }
        Ok(Self { fonts })
    }

    fn get_font(&self, s: &str) -> Option<&Font> {
        self.fonts.get(s)
    }
}

#[derive(Educe, Clone)]
#[educe(Debug)]
struct TextBlock {
    matrix: TransformMatrix,
    font_size: f32,
    font_name: Option<String>,

    char_spacing: f32,              // Tc
    word_spacing: f32,              // Tw
    horiz_scaling: f32,             // Th
    leading: f32,                   // Tl
    render_mode: TextRenderingMode, // Tmode
    rise: f32,                      // Trise
    knockout: bool,                 // Tk
}

impl TextBlock {
    pub fn new() -> Self {
        Self {
            matrix: TransformMatrix::identity(),
            font_size: 0.0,
            font_name: None,

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
        self.matrix = TransformMatrix::identity();
    }

    fn set_font(&mut self, name: &NameOfDict, size: f32) {
        self.font_size = size;
        self.font_name = Some(name.0.to_owned());
    }

    fn move_text_position(&mut self, p: Point) {
        let matrix: Transform = self.matrix.into();
        self.matrix = matrix.pre_translate(p.x, p.y).into();
    }

    fn set_text_matrix(&mut self, m: TransformMatrix) {
        self.matrix = m;
    }

    fn move_right(&mut self, n: f32) {
        let matrix: Transform = self.matrix.into();
        let tx = -n * 0.001 * self.font_size;
        let ty = 0.0;
        let dx = matrix.sx * tx + matrix.kx * ty;
        let dy = matrix.ky * tx + matrix.sy * ty;
        debug!("move_right: {} {} {} {} {}", n, tx, ty, dx, dy);
        debug!("matrix: {:?}", matrix);
        self.matrix = matrix.post_translate(dx, dy).into();
        debug!("matrix after move right: {:?}", self.matrix);
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
