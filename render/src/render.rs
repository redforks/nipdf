use crate::{
    into_skia::to_skia_color,
    shading::{build_shading, Axial, Radial, Shading},
    IntoSkia, PageDimension, RenderOption, RenderOptionBuilder,
};
use anyhow::Result as AnyResult;
use educe::Educe;
use either::Either::{self, Left, Right};
use euclid::{default::Size2D, Length, Scale, Transform2D};
use image::RgbaImage;
use log::{debug, info};
use nipdf::{
    file::{
        paint::fonts::{FontCache, GlyphRender, PathSink},
        GraphicsStateParameterDict, PageContent, Rectangle, ResourceDict, XObjectDict, XObjectType,
    },
    function::Domain,
    graphics::{
        color_space::{ColorSpace, ColorSpaceTrait},
        parse_operations,
        pattern::{PatternType, ShadingPatternDict, TilingPatternDict},
        trans::{
            f_flip, image_to_user_space, move_text_space_pos, move_text_space_right, GlyphLength,
            GlyphSpace, GlyphToTextSpace, GlyphToUserSpace, ImageToDeviceSpace, PatternSpace,
            PatternToUserSpace, TextPoint, TextSpace, TextToUserSpace, ThousandthsOfText,
            UserToDeviceSpace, UserToLogicDeviceSpace, UserToUserSpace,
        },
        ColorArgs, ColorArgsOrName, LineCapStyle, LineJoinStyle, NameOfDict, Operation, Point,
        RenderingIntent, TextRenderingMode,
    },
    object::{ImageMetadata, InlineImage, Object, PdfObject, TextStringOrNumber},
};
use nom::{combinator::eof, sequence::terminated};
use prescript::Name;
use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use tiny_skia::{
    Color as SkiaColor, FillRule, FilterQuality, Mask, MaskType, Paint, Path as SkiaPath,
    PathBuilder, Pixmap, PixmapPaint, PixmapRef, Rect, Stroke, StrokeDash, Transform,
};

trait CloneOrMove {
    type Target;

    fn clone_or_move(self) -> Self::Target;
}

impl CloneOrMove for SkiaPath {
    type Target = SkiaPath;

    fn clone_or_move(self) -> Self::Target {
        self
    }
}

impl<T: Clone> CloneOrMove for &T {
    type Target = T;

    fn clone_or_move(self) -> Self::Target {
        self.clone()
    }
}

#[derive(Clone, Debug)]
enum PaintCreator {
    Color(SkiaColor),
    Gradient((Shading, UserToLogicDeviceSpace)),
    Tile((Pixmap, PatternToUserSpace)),
}

impl PaintCreator {
    fn create(&self, alpha: f32) -> Cow<'_, Paint<'_>> {
        match self {
            PaintCreator::Color(c) => {
                let mut r = Paint::default();
                let mut c = *c;
                c.set_alpha(alpha);
                r.set_color(c);
                Cow::Owned(r)
            }

            PaintCreator::Gradient((pattern, matrix)) => Cow::Owned(Paint {
                shader: pattern.to_skia(matrix, alpha).unwrap(),
                ..Default::default()
            }),

            PaintCreator::Tile((p, matrix)) => {
                let mut r = Paint::default();
                let transform =
                    f_flip::<PatternSpace, PatternSpace>(p.height() as f32).then(matrix);
                r.shader = tiny_skia::Pattern::new(
                    p.as_ref(),
                    tiny_skia::SpreadMode::Repeat,
                    FilterQuality::Bicubic,
                    alpha,
                    transform.into_skia(),
                );
                Cow::Owned(r)
            }
        }
    }
}

type MaskEntry = (Rc<SkiaPath>, Rc<RefCell<Mask>>);

/// Keep last N records of (Path, Mask), reuse the mask if path is the same.
#[derive(Debug)]
struct MaskCache<const N: usize> {
    recents: VecDeque<MaskEntry>,
}

impl<const N: usize> MaskCache<N> {
    pub fn new() -> Self {
        Self {
            recents: VecDeque::with_capacity(N),
        }
    }

    /// Update current mask by intersect a new path on current mask.
    ///
    /// If current mask is None, create the mask, and save into cache.
    ///
    /// If current mask is not None, intersect current path with the new path.
    /// iterate all cached records, if path is the same, return it.
    ///
    /// If not found, intersect the new path with current mask, and save into cache.
    pub fn update(
        &mut self,
        p: SkiaPath,
        current: Option<MaskEntry>,
        rule: FillRule,
        create_mask: impl FnOnce() -> Mask,
    ) -> MaskEntry {
        debug_assert!(!p.is_empty());

        let (new_path, cur_mask) = match current {
            None => (p.clone(), None),
            Some(cur) => {
                let mut r = PathBuilder::new();
                r.push_path(&cur.0);
                r.push_path(&p);
                (r.finish().unwrap(), Some(Rc::clone(&cur.1)))
            }
        };

        for (i, e) in self.recents.iter().enumerate() {
            if e.0.as_ref() == &new_path {
                let entry = self.recents.swap_remove_back(i).unwrap();
                self.recents.push_front(entry.clone());
                return entry;
            }
        }

        let mut mask: Mask = cur_mask.map_or_else(create_mask, |m| m.borrow().clone());
        mask.intersect_path(&p, rule, true, Transform::identity());
        let entry = (Rc::new(new_path), Rc::new(RefCell::new(mask)));
        if self.recents.len() == N {
            self.recents.pop_back();
        }
        self.recents.push_front(entry.clone());
        entry
    }
}

#[derive(Debug, Clone, Educe)]
#[educe(Default)]
struct ColorState {
    // apply before `self.paint` if not null
    background_paint: Option<PaintCreator>,

    #[educe(Default(expression = "PaintCreator::Color(SkiaColor::BLACK)"))]
    paint: PaintCreator,
    #[educe(Default(expression = "ColorSpace::DeviceRGB"))]
    color_space: ColorSpace<f32>,
    #[educe(Default = 1.0f32)]
    alpha: f32,
    #[educe(Default = true)]
    alpha_is_shape: bool,
}

impl ColorState {
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }

    pub fn set_color_args(&mut self, color_args: impl AsRef<[f32]>) {
        let color = to_skia_color(&self.color_space, color_args.as_ref());
        self.set_color(color);
    }

    pub fn set_color(&mut self, color: SkiaColor) {
        self.set_paint(PaintCreator::Color(color), None);
    }

    pub fn set_paint(&mut self, paint: PaintCreator, background_color: Option<SkiaColor>) {
        self.background_paint = background_color.map(PaintCreator::Color);
        self.paint = paint;
    }

    pub fn alpha(&self) -> f32 {
        if self.alpha_is_shape { 1.0 } else { self.alpha }
    }

    /// If background_paint not null, stroke using it before use self.paint
    pub fn stroke(
        &self,
        canvas: &mut Pixmap,
        path: &SkiaPath,
        stroke: &Stroke,
        transform: Transform,
        mask: Option<&Mask>,
    ) {
        if let Some(paint) = &self.background_paint {
            canvas.stroke_path(path, &paint.create(self.alpha()), stroke, transform, mask);
        }
        canvas.stroke_path(
            path,
            &self.paint.create(self.alpha()),
            stroke,
            transform,
            mask,
        );
    }

    pub fn create_paint(&self) -> Cow<'_, Paint<'_>> {
        self.paint.create(self.alpha())
    }

    /// If background_paint not null, fill using it before use self.paint
    pub fn fill(
        &self,
        canvas: &mut Pixmap,
        path: &SkiaPath,
        fill_rule: FillRule,
        transform: Transform,
        mask: Option<&Mask>,
    ) {
        if let Some(paint) = &self.background_paint {
            canvas.fill_path(
                path,
                &paint.create(self.alpha()),
                fill_rule,
                transform,
                mask,
            );
        }
        canvas.fill_path(
            path,
            &self.paint.create(self.alpha()),
            fill_rule,
            transform,
            mask,
        );
    }

    fn set_alpha_is_shape(&mut self, v: bool) {
        self.alpha_is_shape = v;
    }
}
#[derive(Debug, Clone)]
pub(super) struct State {
    dimension: PageDimension,
    ctm: UserToLogicDeviceSpace,
    user_to_device: UserToDeviceSpace,
    stroke: Stroke,
    mask: Option<MaskEntry>,
    mask_cache: Rc<RefCell<MaskCache<4>>>,
    text_object: TextObject,
    stroke_state: ColorState,
    fill_state: ColorState,
}

impl State {
    /// height: height in user space coordinate
    fn new(option: &RenderOption) -> Self {
        let mut r = Self {
            dimension: option.dimension,
            user_to_device: UserToDeviceSpace::identity(),
            ctm: UserToLogicDeviceSpace::identity(),
            stroke: Stroke::default(),
            mask: None,
            mask_cache: Rc::new(RefCell::new(MaskCache::new())),
            text_object: TextObject::new(),
            stroke_state: ColorState::default(),
            fill_state: ColorState::default(),
        };

        r.set_ctm(UserToLogicDeviceSpace::identity());
        r.set_line_cap(LineCapStyle::default());
        r.set_line_join(LineJoinStyle::default());
        r.set_miter_limit(10.0);
        r.set_dash_pattern(&[], 0.0);
        r.set_render_intent(RenderingIntent::default());

        r
    }

    fn update_user_to_device(&mut self) {
        self.user_to_device = self.ctm.then(&self.dimension.logic_device_to_device());
        debug!("ctm to {:?}", self.ctm);
        debug!("user_to_device to {:?}", self.user_to_device);
    }

    fn set_ctm(&mut self, ctm: UserToLogicDeviceSpace) {
        self.ctm = self.dimension.transform.then(&ctm);
        self.update_user_to_device();
    }

    fn concat_ctm(&mut self, ctm: UserToUserSpace) {
        self.ctm = ctm.then(&self.ctm);
        self.update_user_to_device();
    }

    fn set_line_width(&mut self, w: f32) {
        self.stroke.width = w;
    }

    fn set_line_cap(&mut self, cap: LineCapStyle) {
        self.stroke.line_cap = cap.into_skia();
    }

    fn set_line_join(&mut self, join: LineJoinStyle) {
        self.stroke.line_join = join.into_skia();
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

    fn get_fill_paint(&self) -> Cow<'_, Paint<'_>> {
        self.fill_state.create_paint()
    }

    fn get_stroke_paint(&self) -> Cow<'_, Paint<'_>> {
        self.stroke_state.create_paint()
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> ImageToDeviceSpace {
        image_to_user_space(img_w, img_h).then(&self.user_to_device)
    }

    fn get_mask(&self) -> Option<Ref<Mask>> {
        self.mask.as_ref().map(|m| m.1.borrow())
    }

    fn set_graphics_state(&mut self, res: &GraphicsStateParameterDict) {
        for key in res.dict().keys() {
            match key.as_str() {
                "LW" => self.set_line_width(res.line_width().unwrap().unwrap()),
                "LC" => self.set_line_cap(res.line_cap().unwrap().unwrap()),
                "LJ" => self.set_line_join(res.line_join().unwrap().unwrap()),
                "ML" => self.set_miter_limit(res.miter_limit().unwrap().unwrap()),
                "RI" => self.set_render_intent(res.rendering_intent().unwrap().unwrap()),
                "TK" => self.set_text_knockout_flag(res.text_knockout_flag().unwrap().unwrap()),
                "FL" => self.set_flatness(res.flatness().unwrap().unwrap()),
                "CA" => self.set_stroke_alpha(res.stroke_alpha().unwrap().unwrap()),
                "ca" => self.set_fill_alpha(res.fill_alpha().unwrap().unwrap()),
                "AIS" => self.set_alpha_is_shape(res.alpha_is_shape().unwrap().unwrap()),
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

    fn update_mask(
        &mut self,
        path: impl CloneOrMove<Target = SkiaPath>,
        rule: FillRule,
        flip_y: bool,
    ) {
        let w = self.dimension.canvas_width();
        let h = self.dimension.canvas_height();
        let new_mask = || {
            let mut r = Mask::new(w, h).unwrap();
            let p = PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap());
            r.fill_path(&p, FillRule::Winding, true, Transform::identity());
            r
        };

        let mut path = path.clone_or_move();
        if flip_y {
            path = path.transform(self.user_to_device.into_skia()).unwrap();
        }

        self.mask = Some(self.mask_cache.borrow_mut().update(
            path,
            self.mask.clone(),
            rule,
            new_mask,
        ));
        // use std::sync::atomic::{AtomicU32, Ordering};
        // static mut IDX: std::sync::atomic::AtomicU32 = AtomicU32::new(0);
        // if let Some(mask) = &self.mask {
        //     dbg!(&mask.0);
        //     mask.1
        //         .borrow()
        //         .save_png(format!("/tmp/mask-{:?}.png", unsafe {
        //             IDX.fetch_add(1, Ordering::Relaxed)
        //         }))
        //         .unwrap();
        // }
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Winding fill rule.
    fn clip_non_zero(&mut self, path: impl CloneOrMove<Target = SkiaPath>, flip_y: bool) {
        self.update_mask(path, FillRule::Winding, flip_y);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Even-Odd fill rule.
    fn clip_even_odd(&mut self, path: impl CloneOrMove<Target = SkiaPath>, flip_y: bool) {
        self.update_mask(path, FillRule::EvenOdd, flip_y);
    }

    fn set_text_knockout_flag(&mut self, knockout: bool) {
        self.text_object.knockout = knockout;
        todo!("text knockout");
    }

    pub fn end_text_object(&mut self) {
        // if exists text clipping path, intersection to current clipping path using Winding fill
        // rule
        let p = self.text_object.text_clipping_path.finish();
        if let Some(p) = p {
            let p = p.to_owned();
            self.clip_non_zero(p, false);
            self.text_object.text_clipping_path.reset();
        }
    }

    fn set_stroke_alpha(&mut self, alpha: f32) {
        self.stroke_state.set_alpha(alpha);
    }

    fn set_fill_alpha(&mut self, alpha: f32) {
        self.fill_state.set_alpha(alpha);
    }

    fn set_alpha_is_shape(&mut self, v: bool) {
        self.stroke_state.set_alpha_is_shape(v);
        self.fill_state.set_alpha_is_shape(v);
    }
}

#[derive(Debug, Clone, Educe)]
#[educe(Default)]
struct Path {
    #[educe(Default(expression = "Either::Left(PathBuilder::new())"))]
    path: Either<PathBuilder, SkiaPath>,
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
        self.curve_to(Point::new(p1.x, p1.y), p2, p3);
    }

    pub fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) {
        self.curve_to(p1, p3, p3);
    }

    pub fn append_rect(&mut self, p: Point, w: f32, h: f32) {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path_builder().push_rect(r.into_skia());
    }

    /// Build path and clear the path builder, return None if path is empty
    pub fn finish(&mut self) -> Option<&SkiaPath> {
        if let Left(_) = self.path {
            let temp = Left(PathBuilder::new());
            let pb = std::mem::replace(&mut self.path, temp).left().unwrap();
            if let Some(p) = pb.finish() {
                self.path = Right(p);
            } else {
                debug!("empty or invalid path");
            }
        }

        match &self.path {
            Left(_) => None,
            Right(p) => Some(p),
        }
    }

    pub fn reset(&mut self) {
        let temp = Left(PathBuilder::new());
        let p = std::mem::replace(&mut self.path, temp);
        self.path = p.right_and_then(|p| Left(p.clear()));
    }

    pub fn clear(&mut self) {
        self.reset();
    }
}

struct SkiaPathSink(PathBuilder);

impl SkiaPathSink {
    fn into_inner(self) -> PathBuilder {
        self.0
    }
}

impl PathSink for SkiaPathSink {
    #[inline]
    fn move_to(&mut self, to: Point) {
        self.0.move_to(to.x, to.y);
    }

    #[inline]
    fn line_to(&mut self, to: Point) {
        self.0.line_to(to.x, to.y);
    }

    #[inline]
    fn quad_to(&mut self, ctrl: Point, to: Point) {
        self.0.quad_to(ctrl.x, ctrl.y, to.x, to.y);
    }

    #[inline]
    fn cubic_to(&mut self, ctrl1: Point, ctrl2: Point, to: Point) {
        self.0
            .cubic_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y);
    }

    #[inline]
    fn close(&mut self) {
        self.0.close();
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Render<'a, 'b, 'c> {
    canvas: &'c mut Pixmap,
    stack: Vec<State>,
    path: Path,
    #[educe(Debug(ignore))]
    font_cache: FontCache<'c, SkiaPathSink>,
    resources: &'c ResourceDict<'a, 'b>,
    dimension: PageDimension,
}

impl<'a, 'b: 'a, 'c> Render<'a, 'b, 'c> {
    pub fn new(
        canvas: &'c mut Pixmap,
        option: RenderOption,
        resources: &'c ResourceDict<'a, 'b>,
    ) -> Self
    where
        'a: 'c,
        'b: 'c,
    {
        let mut state = if let Some(state) = option.state {
            state
        } else {
            State::new(&option)
        };

        if let Some(rect) = option.crop {
            state.clip_non_zero(PathBuilder::from_rect(rect.into_skia()), true);
        }

        Self {
            canvas,
            stack: vec![state],
            path: Path::default(),
            font_cache: FontCache::new(resources).unwrap(),
            resources,
            dimension: option.dimension,
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
        if self.stack.pop().is_none() {
            // some file contains unpaired q/Q operations
            info!("pop empty state stack");
        }
    }

    fn current_mut(&mut self) -> &mut State {
        self.stack.last_mut().unwrap()
    }

    fn text_object(&self) -> &TextObject {
        &self.stack.last().unwrap().text_object
    }

    fn text_object_mut(&mut self) -> &mut TextObject {
        &mut self.current_mut().text_object
    }

    pub(crate) fn exec(&mut self, op: Operation) {
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
            Operation::SetFont(name, size) => self.text_object_mut().set_font(name, size),
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
            Operation::MoveToStartOfNextLine => self.move_to_start_of_next_line(),

            // Text Showing Operations
            Operation::ShowText(text) => self.show_text(text.to_bytes().unwrap()),
            Operation::MoveToNextLineAndShowText(text) => {
                self.move_to_start_of_next_line();
                self.show_text(text.to_bytes().unwrap());
            }
            Operation::ShowTexts(texts) => self.show_texts(&texts),

            // Color Operations
            Operation::SetStrokeColorSpace(args) => {
                self.current_mut().stroke_state.color_space =
                    ColorSpace::from_args(&args, self.resources.resolver(), Some(self.resources))
                        .unwrap()
            }
            Operation::SetFillColorSpace(args) => {
                self.current_mut().fill_state.color_space =
                    ColorSpace::from_args(&args, self.resources.resolver(), Some(self.resources))
                        .unwrap()
            }
            Operation::SetStrokeColor(args) => self.set_color_args(Self::stroke_color_state, args),
            Operation::SetStrokeGray(color) => {
                self.set_color_and_space(Self::stroke_color_state, ColorSpace::DeviceGray, &color)
            }
            Operation::SetStrokeCMYK(color) => {
                self.set_color_and_space(Self::stroke_color_state, ColorSpace::DeviceCMYK, &color)
            }
            Operation::SetStrokeRGB(color) => {
                self.set_color_and_space(Self::stroke_color_state, ColorSpace::DeviceRGB, &color)
            }
            Operation::SetStrokeColorOrWithPattern(color_or_name) => self
                .set_color_or_pattern(Self::stroke_color_state, &color_or_name)
                .unwrap(),
            Operation::SetFillColor(args) => self.set_color_args(Self::fill_color_state, args),
            Operation::SetFillGray(color) => {
                self.set_color_and_space(Self::fill_color_state, ColorSpace::DeviceGray, &color)
            }
            Operation::SetFillCMYK(color) => {
                self.set_color_and_space(Self::fill_color_state, ColorSpace::DeviceCMYK, &color)
            }
            Operation::SetFillRGB(color) => {
                self.set_color_and_space(Self::fill_color_state, ColorSpace::DeviceRGB, &color)
            }
            Operation::SetFillColorOrWithPattern(color_or_name) => self
                .set_color_or_pattern(Self::fill_color_state, &color_or_name)
                .unwrap(),

            // Shading Operation
            Operation::PaintShading(name) => self.paint_shading(name).unwrap(),

            // XObject Operation
            Operation::PaintXObject(name) => self.paint_x_object(&name).unwrap(),

            // Marked Content Operations
            Operation::DesignateMarkedContentPoint(_)
            | Operation::DesignateMarkedContentPointWithProperties(_, _)
            | Operation::BeginMarkedContent(_)
            | Operation::BeginMarkedContentWithProperties(_, _)
            | Operation::EndMarkedContent => {
                debug!("not implemented: {:?}", op);
            }

            // Type3 Extra Operations
            // Define something already known in FontDict, can safely ignored
            Operation::SetGlyphWidth(_) => {}
            Operation::SetGlyphWidthAndBoundingBox(_, _, _) => {}

            Operation::PaintInlineImage(inline_image) => {
                self.paint_inline_image(inline_image).unwrap()
            }

            _ => todo!("{:?}", op),
        }
    }

    fn move_to_start_of_next_line(&mut self) {
        let leading = self.stack.last().unwrap().text_object.leading;
        self.text_object_mut()
            .move_text_position(TextPoint::new(0.0, -leading));
    }

    fn set_color_args(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> &mut ColorState,
        args: ColorArgs,
    ) {
        let state = get_state(self);
        state.set_color_args(&args);
    }

    fn set_color_and_space(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> &mut ColorState,
        cs: ColorSpace<f32>,
        color: &[f32],
    ) {
        let state = get_state(self);
        state.color_space = cs;
        state.set_color_args(color);
    }

    fn stroke(&mut self) {
        if let Some(p) = self.path.finish() {
            let state = self.stack.last().unwrap();
            let stroke = state.get_stroke();
            state.stroke_state.stroke(
                self.canvas,
                p,
                stroke,
                state.user_to_device.into_skia(),
                state.get_mask().as_deref(),
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
        if let Some(p) = self.path.finish() {
            state.fill_state.fill(
                self.canvas,
                p,
                fill_rule,
                state.user_to_device.into_skia(),
                state.get_mask().as_deref(),
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

    fn load_image_as_mask(mut img: RgbaImage, state: &State, s_mask: bool) -> AnyResult<Mask> {
        let paint = PixmapPaint {
            quality: FilterQuality::Nearest,
            ..Default::default()
        };

        let mut canvas = Pixmap::new(
            state.dimension.canvas_width(),
            state.dimension.canvas_height(),
        )
        .unwrap();
        img.pixels_mut()
            .for_each(|p| p[3] = if s_mask { p[0] } else { !p[0] });

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

    fn paint_inline_image(&mut self, inline_image: InlineImage) -> AnyResult<()> {
        let state = self.stack.last().unwrap();
        let meta = inline_image.meta();
        let img = inline_image
            .image(self.resources.resolver(), self.resources)?
            .into_rgba8();

        if meta.image_mask()? {
            let domain = meta.decode()?.unwrap().0[0];
            let mask_reversed = domain.start > domain.end;
            let mask = Self::load_image_as_mask(img, state, mask_reversed)?;
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
            return Ok(());
        }

        let paint = PixmapPaint {
            opacity: state.fill_state.alpha(),
            ..Default::default()
        };
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        let state_mask = state.get_mask();
        self.canvas.draw_pixmap(
            0,
            0,
            img,
            &paint,
            state.image_transform(img.width(), img.height()).into_skia(),
            state_mask.as_deref(),
        );
        Ok(())
    }

    fn paint_image_x_object(&mut self, x_object: &XObjectDict<'a, '_>) -> AnyResult<()> {
        fn load_image<'a, 'b>(
            image_dict: &XObjectDict<'a, 'b>,
            resources: &ResourceDict<'a, 'b>,
        ) -> RgbaImage {
            let image = image_dict
                .as_stream()
                .expect("Only Image XObject supported");
            image
                .decode_image(resources.resolver(), Some(resources))
                .unwrap()
                .into_rgba8()
        }

        let state = self.stack.last().unwrap();

        if x_object.image_mask()? {
            let is_invert = if let Some(decode) = x_object.decode()? {
                let domain = decode.0[0];
                domain.start > domain.end
            } else {
                false
            };
            let x_object = x_object.as_stream()?;
            let img = x_object.decode_image(self.resources.resolver(), Some(self.resources))?;
            let mask = Self::load_image_as_mask(img.into_rgba8(), state, is_invert)?;
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
            return Ok(());
        }

        let s_mask = x_object.s_mask()?.map(|s_mask| {
            let s_mask = s_mask.as_stream().unwrap();
            let img = s_mask
                .decode_image(self.resources.resolver(), Some(self.resources))
                .unwrap();
            Self::load_image_as_mask(img.into_rgba8(), state, true).unwrap()
        });

        let paint = PixmapPaint {
            opacity: state.fill_state.alpha(),
            quality: if x_object.interpolate()? {
                FilterQuality::Bilinear
            } else {
                FilterQuality::Nearest
            },
            ..Default::default()
        };
        let img = load_image(x_object, self.resources);
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        let state_mask = state.get_mask();
        self.canvas.draw_pixmap(
            0,
            0,
            img,
            &paint,
            state.image_transform(img.width(), img.height()).into_skia(),
            s_mask.as_ref().or(state_mask.as_deref()),
        );
        Ok(())
    }

    /// Paint form x_object.
    ///
    /// 1. Create a sub Render to paint the form, set transparent as background
    /// 1. Clone current state to sub render to use exist state
    /// 1. Sub render concatenate form's Matrix to ctm
    /// 1. Assert form b_box start point is (0, 0), because I'm not sure what
    /// will happen, wait for an example pdf file that b_box start point is not (0, 0)
    /// 1. Paints the graphics objects specified in the form object's stream in sub render.
    /// 1. Paint the rendered image on parent render
    fn paint_form_x_object(&mut self, x_object: &XObjectDict<'a, 'b>) -> AnyResult<()> {
        debug!("Render form");

        let form = x_object.as_form()?;
        let matrix = form.matrix()?;
        let b_box = form.b_box()?;
        let stream = x_object.as_stream()?;
        let stream = stream.decode(self.resources.resolver())?;
        let content = PageContent::new(vec![stream.into_owned()]);
        let resources = form.resources()?;
        let resources = resources.as_ref().unwrap_or(self.resources);

        let state = self.stack.last().unwrap();
        let mut inner_state = state.clone();
        let ctm = matrix.then(&state.ctm).with_destination().with_source();
        inner_state.set_ctm(ctm);
        let mut render = Render::new(
            self.canvas,
            RenderOptionBuilder::default()
                .dimension(self.dimension)
                .crop(Some(b_box))
                .background_color(SkiaColor::TRANSPARENT)
                .state(inner_state)
                .build(),
            resources,
        );
        content
            .operations()
            .into_iter()
            .for_each(|op| render.exec(op));

        Ok(())
    }

    /// Paints the specified XObject. Only XObjectType::Image supported
    fn paint_x_object(&mut self, nm: &NameOfDict) -> AnyResult<()> {
        let x_objects = self.resources.x_object()?;
        let x_object = &x_objects[&nm.0];

        match x_object.subtype()? {
            XObjectType::Image => self.paint_image_x_object(x_object),
            XObjectType::Form => self.paint_form_x_object(x_object),
            t => todo!("{:?}", t),
        }
    }

    fn paint_axial(&mut self, axial: Axial) -> Result<(), anyhow::Error> {
        let b_box = axial.b_box;

        let state = self.stack.last().unwrap();
        let ctm = state.user_to_device.into_skia();
        let (shader_ctm, fill_ctm, path) = if let Some(b_box) = b_box {
            (
                Transform::identity(),
                ctm,
                PathBuilder::from_rect(b_box.into_skia()),
            )
        } else if let Some((path, _)) = &state.mask {
            (ctm, Transform::identity(), (**path).clone())
        } else {
            (
                ctm,
                Transform::identity(),
                PathBuilder::from_rect(
                    Rectangle::from_xywh(
                        0.,
                        0.,
                        self.device_width() as f32,
                        self.device_height() as f32,
                    )
                    .into_skia(),
                ),
            )
        };

        if let Some(shader) = axial.to_skia(shader_ctm, state.fill_state.alpha()) {
            let paint = Paint {
                shader,
                ..Default::default()
            };
            self.canvas.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                fill_ctm,
                state.get_mask().as_deref(),
            );
        }
        Ok(())
    }

    fn paint_radial(&mut self, radial: &Radial) -> AnyResult<()> {
        let Domain { start: t0, end: t1 } = radial.domain;
        let (x0, y0) = (radial.start.point.x, radial.start.point.y);
        let (x1, y1) = (radial.end.point.x, radial.end.point.y);
        let r0 = radial.start.r;
        let r1 = radial.end.r;
        let state = self.stack.last().unwrap();
        let ctm = state.user_to_device;
        let mask = state.get_mask();
        let mut paint = Paint::default();
        let stroke = Stroke::default();

        let circle = |t: f32| {
            let s = (t - t0) / (t1 - t0);
            let x = s.mul_add(x1 - x0, x0);
            let y = s.mul_add(y1 - y0, y0);
            let r = s.mul_add(r1 - r0, r0);
            (x, y, r)
        };

        // calc how many steps to paint: get start circle point, and end circle point, calc distance
        // between them, then calc how many steps to paint
        let (cx1, cy1, cr1) = circle(0.0);
        let (cx2, cy2, cr2) = circle(1.0);
        let (cx1, cy1) = ctm.transform_point((cx1, cy1).into()).into();
        let (cx2, cy2) = ctm.transform_point((cx2, cy2).into()).into();
        let d = (cx1 - cx2).hypot(cy1 - cy2);
        let steps = if d < 1.0 {
            let (cx1, cy1) = ctm.transform_point((0., 0.).into()).into();
            let (cx2, cy2) = ctm.transform_point((cr1 + cr2, 0.).into()).into();
            (cx1 - cx2).hypot(cy1 - cy2) * 2.
        } else {
            d / 2.0
        }
        .ceil() as usize;
        let steps = steps.max(10);

        let ctm = ctm.into_skia();
        if radial.extend.end() {
            let c = radial.function.call(&[1.0])?;
            let mut c = radial.color_space.to_rgba(c.as_slice());
            c[3] = state.fill_state.alpha();
            paint.set_color(SkiaColor::from_rgba(c[0], c[1], c[2], c[3]).unwrap());
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
                state.get_mask().as_deref(),
            );
        }

        if radial.extend.begin() && radial.start.r > 0.0 {
            let (x, y) = (radial.start.point.x, radial.start.point.y);
            let r = radial.start.r;
            let c = radial.function.call(&[0.0])?;
            let c = radial.color_space.to_rgba(c.as_slice());
            paint.set_color(SkiaColor::from_rgba(c[0], c[1], c[2], c[3]).unwrap());
            let path = PathBuilder::from_circle(x, y, r).unwrap();
            let path = path.transform(ctm).unwrap();
            self.canvas.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                state.get_mask().as_deref(),
            );
        }

        for t in 0..=steps {
            let t = t as f32 / steps as f32;
            let (x, y, r) = circle(t);
            let c = radial.function.call(&[t][..])?;
            let c = radial.color_space.to_rgba(c.as_slice());

            let Some(path) = PathBuilder::from_circle(x, y, r) else {
                continue;
            };
            let path = path.transform(ctm).unwrap();
            paint.set_color(SkiaColor::from_rgba(c[0], c[1], c[2], c[3]).unwrap());
            self.canvas.stroke_path(
                &path,
                &paint,
                &stroke,
                Transform::identity(),
                mask.as_deref(),
            );
        }

        Ok(())
    }

    fn paint_shading(&mut self, nm: NameOfDict) -> AnyResult<()> {
        let shading = self.resources.shading()?;
        let shading = &shading[&nm.0];
        match build_shading(shading, self.resources)? {
            Some(Shading::Radial(radial)) => self.paint_radial(&radial),
            Some(Shading::Axial(axial)) => self.paint_axial(axial),
            None => Ok(()),
        }
    }

    fn fill_color_state(&mut self) -> &mut ColorState {
        &mut self.current_mut().fill_state
    }

    fn stroke_color_state(&mut self) -> &mut ColorState {
        &mut self.current_mut().stroke_state
    }

    fn set_color_or_pattern(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> &mut ColorState,
        color_or_name: &ColorArgsOrName,
    ) -> AnyResult<()> {
        match color_or_name {
            ColorArgsOrName::Name((name, color_args)) => {
                let color = color_args
                    .as_ref()
                    .map(|args| to_skia_color(&get_state(self).color_space, args.as_ref()));
                let pattern = self.resources.pattern()?;
                let pattern = &pattern[name];
                match pattern.pattern_type()? {
                    PatternType::Tiling => {
                        let dimension = Size2D::new(
                            self.dimension.canvas_width() as f32,
                            self.dimension.canvas_height() as f32,
                        );
                        Self::tiling_pattern(
                            &dimension,
                            get_state(self),
                            pattern.tiling_pattern()?,
                            color,
                        )
                    }
                    PatternType::Shading => {
                        if let Some((paint, background_color)) =
                            self.shading_pattern(pattern.shading_pattern()?)?
                        {
                            let color_state = get_state(self);
                            color_state.set_paint(paint, background_color);
                        }
                        Ok(())
                    }
                }
            }
            ColorArgsOrName::Color(args) => {
                let state = get_state(self);
                state.set_color_args(args);
                Ok(())
            }
        }
    }

    fn shading_pattern(
        &mut self,
        pattern: ShadingPatternDict<'a, 'b>,
    ) -> AnyResult<Option<(PaintCreator, Option<SkiaColor>)>> {
        struct RestoreState<F>(Option<F>)
        where
            F: FnOnce();
        impl<F> Drop for RestoreState<F>
        where
            F: FnOnce(),
        {
            fn drop(&mut self) {
                if let Some(f) = self.0.take() {
                    f()
                }
            }
        }

        let resources = self.resources;
        let _restore = if let Some(ext_g_state) = pattern.ext_g_state()? {
            self.push();
            self.current_mut().set_graphics_state(&ext_g_state);
            Some(RestoreState(Some(|| self.pop())))
        } else {
            None
        };

        let shading = pattern.shading()?;
        // assert!(shading.b_box()?.is_none(), "TODO: support BBox of shading");
        let background_color = if let Some(args) = shading.background()? {
            let cs = shading.color_space()?;
            let cs = ColorSpace::from_args(&cs, resources.resolver(), Some(resources)).unwrap();
            Some(to_skia_color(&cs, args.as_ref()))
        } else {
            None
        };

        Ok(match build_shading(&shading, resources)? {
            Some(shading) => Some((shading, pattern.matrix()?)),
            None => return Ok(None),
        }
        .map(|shader| (PaintCreator::Gradient(shader), background_color)))
    }

    fn tiling_pattern(
        canvas_size: &Size2D<f32>,
        color_state: &mut ColorState,
        tile: TilingPatternDict<'a, 'b>,
        color: Option<SkiaColor>,
    ) -> AnyResult<()>
    where
        'a: 'b,
    {
        let stream: &Object = tile.resolver().resolve(tile.id().unwrap())?;
        let stream = stream.stream()?;
        let bytes = stream.decode(tile.resolver())?;
        let (_, ops) = terminated(parse_operations, eof)(bytes.as_ref()).unwrap();
        let b_box = tile.b_box()?;
        assert_eq!(b_box.width(), tile.x_step()?, "x_step not supported");
        assert_eq!(b_box.height(), tile.y_step()?, "y_step not supported");
        let mut zoom = 1.0f32;
        let (mut w, mut h) = (b_box.width(), b_box.height());
        let mut matrix = tile.matrix()?;
        while w > canvas_size.width && h > canvas_size.height {
            w /= 2.0;
            h /= 2.0;
            zoom /= 2.0;
            matrix = matrix.then_scale(2.0, 2.0);
        }

        let resources = tile.resources()?;
        let option = RenderOptionBuilder::default()
            .zoom(zoom)
            .page_box(&b_box, 0)
            .build();
        let mut canvas = option.create_canvas();
        let mut render = Render::new(&mut canvas, option, &resources);
        if let Some(color) = color {
            // set color used for paint matrix image
            color_state.set_color(color);
        }
        ops.into_iter().for_each(|op| render.exec(op));
        drop(render);
        color_state.paint = PaintCreator::Tile((canvas, matrix));
        Ok(())
    }

    fn gen_glyph_path(glyph_render: &dyn GlyphRender<SkiaPathSink>, gid: u16) -> PathBuilder {
        let mut sink = SkiaPathSink(PathBuilder::new());
        glyph_render.render(gid, &mut sink).unwrap();
        sink.into_inner()
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
                    state.get_mask().as_deref(),
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
                    state.get_mask().as_deref(),
                );
            }
            TextRenderingMode::FillAndStroke => {
                canvas.fill_path(
                    &path,
                    &state.get_fill_paint(),
                    FillRule::Winding,
                    trans,
                    state.get_mask().as_deref(),
                );
                canvas.stroke_path(
                    &path,
                    &state.get_stroke_paint(),
                    state.get_stroke(),
                    trans,
                    state.get_mask().as_deref(),
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
        if text_object.render_mode == TextRenderingMode::Invisible {
            return;
        }

        let font = self
            .font_cache
            .get_font(text_object.font_name.as_ref().unwrap())
            .unwrap();
        debug!(
            "font: {}, type: {:?}",
            text_object.font_name.as_ref().unwrap(),
            font.font_type()
        );
        let op = self
            .font_cache
            .get_op(self.text_object().font_name.as_ref().unwrap())
            .unwrap();
        let state = self.stack.last().unwrap();
        let mut text_object = state.text_object.clone();
        text_object.set_units_per_em(op.units_per_em() as f32);
        let user_to_device = state.user_to_device.into_skia();

        if let Some(type3_font) = font.as_type3() {
            let font_matrix = type3_font.matrix().unwrap();
            let resources = type3_font.resources().unwrap();
            let mut render = Render::new(
                self.canvas,
                RenderOptionBuilder::default()
                    .dimension(self.dimension)
                    .background_color(SkiaColor::TRANSPARENT)
                    .state(state.clone())
                    .build(),
                resources.as_ref().unwrap_or(self.resources),
            );

            for ch in op.decode_chars(text) {
                render.current_mut().set_ctm(
                    text_object
                        .type3_runtime_matrix(&font_matrix)
                        .then(&state.ctm)
                        .with_destination()
                        .with_source(),
                );
                if let Some(glyph) = type3_font.get_glyph(op.char_to_gid(ch)) {
                    for op in glyph.operations() {
                        render.exec(op.clone());
                    }
                }

                text_object.move_to_next_pos(op.char_width(ch), ch == 32);
            }
        } else {
            let glyph_render = self
                .font_cache
                .get_glyph_render(self.text_object().font_name.as_ref().unwrap())
                .unwrap();
            let mut text_clip_path = Path::default();

            for ch in op.decode_chars(text) {
                let path = Self::gen_glyph_path(glyph_render, op.char_to_gid(ch));
                if !path.is_empty() {
                    let path = path.finish().unwrap();
                    // pre transform path to user space, render_glyph() will zoom line_width,
                    // pdf line_width state is in user space, but skia line_width is in device
                    // space so we need to transform path to user space,
                    // and zoom line_width in device space
                    let path = path
                        .transform(text_object.runtime_matrix().into_skia())
                        .unwrap();

                    Self::render_glyph(
                        self.canvas,
                        &mut text_clip_path,
                        state,
                        path,
                        text_object.render_mode,
                        user_to_device,
                    );
                }

                text_object.move_to_next_pos(op.char_width(ch), ch == 32);
            }

            if let Some(text_clip_path) = text_clip_path.finish() {
                self.text_object_mut()
                    .text_clipping_path
                    .path_builder()
                    .push_path(text_clip_path);
            }
        }
        self.current_mut().text_object = text_object;
    }

    fn show_texts(&mut self, texts: &[TextStringOrNumber]) {
        for t in texts {
            match t {
                TextStringOrNumber::TextString(s) => self.show_text(s.to_bytes().unwrap()),
                TextStringOrNumber::Number(n) => {
                    self.text_object_mut().adjust_tj(*n);
                }
            }
        }
    }

    fn end_text(&mut self) {
        self.current_mut().end_text_object();
    }
}

trait ToTextSpaceLength {
    fn to_text_space_length(self) -> Length<f32, TextSpace>;
}

impl ToTextSpaceLength for Length<f32, TextSpace> {
    fn to_text_space_length(self) -> Length<f32, TextSpace> {
        self
    }
}

impl ToTextSpaceLength for Length<f32, ThousandthsOfText> {
    fn to_text_space_length(self) -> Length<f32, TextSpace> {
        Length::new(self.0 / 1000.0)
    }
}

#[derive(Educe, Clone)]
#[educe(Debug)]
struct TextObject {
    matrix: TextToUserSpace,
    line_matrix: TextToUserSpace,
    font_size: f32,
    font_name: Option<Name>,
    text_clipping_path: Path,
    // 1 / units_per_em
    em_ratio: Scale<f32, GlyphSpace, TextSpace>,

    char_spacing: Length<f32, TextSpace>, // Tc
    word_spacing: Length<f32, TextSpace>, // Tw
    // Th, divide by 100, 100 to be 1.0 for example
    horiz_scaling: f32,
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
            em_ratio: Scale::new(1.0 / 1000.0),

            char_spacing: Length::new(0.0),
            word_spacing: Length::new(0.0),
            horiz_scaling: 1.0,
            leading: 0.0,
            render_mode: TextRenderingMode::Fill,
            rise: 0.0,
            knockout: true,
        }
    }

    pub fn type3_runtime_matrix(&self, font_matrix: &GlyphToTextSpace) -> GlyphToUserSpace {
        font_matrix
            .then_scale(self.font_size * self.horiz_scaling.abs(), self.font_size)
            .then(&self.matrix)
    }

    pub fn runtime_matrix(&self) -> GlyphToUserSpace {
        Transform2D::scale(self.em_ratio.0, self.em_ratio.0)
            .then_scale(self.font_size * self.horiz_scaling, self.font_size)
            .then(&self.matrix)
    }

    fn reset(&mut self) {
        self.matrix = TextToUserSpace::identity();
        self.line_matrix = TextToUserSpace::identity();
    }

    fn set_font(&mut self, nm: NameOfDict, size: f32) {
        self.font_size = size;
        self.font_name = Some(nm.0);
    }

    fn set_units_per_em(&mut self, units_per_em: f32) {
        self.em_ratio = Scale::new(1.0 / units_per_em);
    }

    fn move_text_position(&mut self, p: TextPoint) {
        let matrix = move_text_space_pos(&self.line_matrix, p);
        self.matrix = matrix;
        self.line_matrix = matrix;
    }

    fn update_horizontal_scale(&mut self) {
        let glyph_manipulate = if self.horiz_scaling == 1.0 {
            Transform2D::<f32, TextSpace, TextSpace>::identity()
        } else {
            Transform2D::<f32, TextSpace, TextSpace>::scale(self.horiz_scaling, 1.0)
        };
        self.matrix = glyph_manipulate.then(&self.matrix);
        self.line_matrix = glyph_manipulate.then(&self.line_matrix);
    }

    fn set_text_matrix(&mut self, m: TextToUserSpace) {
        self.matrix = m;
        self.line_matrix = m;
        self.update_horizontal_scale();
    }

    fn move_to_next_pos(&mut self, glyph_width: GlyphLength, word_boundary: bool) {
        let mut w = glyph_width * self.em_ratio * self.font_size + self.char_spacing;
        if word_boundary {
            w += self.word_spacing;
        }
        self.matrix = move_text_space_right(&self.matrix, w);
    }

    fn adjust_tj(&mut self, tj: Length<f32, ThousandthsOfText>) {
        let n = tj * self.font_size * Scale::new(1.0 / 1000.0);
        self.matrix = move_text_space_right(&self.matrix, -n);
    }

    fn set_character_spacing(&mut self, spacing: Length<f32, TextSpace>) {
        self.char_spacing = spacing;
    }

    fn set_word_spacing(&mut self, spacing: Length<f32, TextSpace>) {
        self.word_spacing = spacing;
    }

    fn set_horizontal_scaling(&mut self, scale: f32) {
        self.horiz_scaling = scale / 100.0;
        self.update_horizontal_scale();
    }

    fn set_leading(&mut self, leading: f32) {
        self.leading = leading;
    }

    fn set_text_rendering_mode(&mut self, mode: TextRenderingMode) {
        self.render_mode = mode;
    }

    fn set_text_rise(&mut self, rise: f32) {
        self.rise = rise;
        if rise != 0. {
            todo!("text rise");
        }
    }
}
