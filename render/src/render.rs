use crate::{
    IntoSkia, PageDimension, RenderOption, RenderOptionBuilder, Result,
    into_skia::to_skia_color,
    shading::{Axial, Radial, Shading, build_shading},
};
use educe::Educe;
use either::Either::{self, Left, Right};
use euclid::{Length, Scale, Transform2D, default::Size2D};
use image::RgbaImage;
use log::{debug, error, info, warn};
use nipdf::{
    file::{
        GraphicsStateParameterDict, PageContent, Rectangle, ResourceDict, XObjectDict, XObjectType,
        paint::fonts::{FontCache, GlyphRender, PathSink},
    },
    function::Domain,
    graphics::{
        ColorArgs, ColorArgsOrName, LineCapStyle, LineJoinStyle, NameOfDict, Operation, Point,
        RenderingIntent, TextRenderingMode,
        color_space::{ColorSpace, ColorSpaceTrait},
        parse_operations,
        pattern::{PatternType, ShadingPatternDict, TilingPatternDict},
        trans::{
            GlyphLength, GlyphSpace, GlyphToTextSpace, GlyphToUserSpace, ImageToDeviceSpace,
            PatternSpace, PatternToUserSpace, TextPoint, TextSpace, TextToUserSpace,
            ThousandthsOfText, UserToDeviceSpace, UserToLogicDeviceSpace, UserToUserSpace, f_flip,
            image_to_user_space, move_text_space_down, move_text_space_pos, move_text_space_right,
        },
    },
    object::{
        ImageMask, ImageMetadata, InlineImage, Object, PdfObjectCore as _, RootPdfObject as _,
        TextStringOrNumber,
    },
};
use num_traits::ToPrimitive;
use prescript::{AnyWhatever, Name, ParserError, cmap::WriteMode};
use snafu::{FromString, OptionExt, ResultExt, ensure_whatever, whatever};
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
use winnow::{Parser as _, combinator::terminated, token::rest};

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
    Tile((Pixmap, PatternToUserSpace, bool)), // 3rd argument: no-repeat
}

impl PaintCreator {
    fn create(&self, alpha: f32) -> Result<Cow<'_, Paint<'_>>> {
        match self {
            PaintCreator::Color(c) => {
                let mut r = Paint::default();
                let mut c = *c;
                c.set_alpha(alpha);
                r.set_color(c);
                Ok(Cow::Owned(r))
            }

            PaintCreator::Gradient((pattern, matrix)) => Ok(Cow::Owned(Paint {
                shader: pattern
                    .to_skia(matrix, alpha)
                    .whatever_context("Create Gradient shader")?,
                ..Default::default()
            })),

            PaintCreator::Tile((p, matrix, no_repeat)) => {
                let mut r = Paint::default();
                let transform =
                    f_flip::<PatternSpace, PatternSpace>(p.height() as f32).then(matrix);
                r.shader = tiny_skia::Pattern::new(
                    p.as_ref(),
                    if *no_repeat {
                        tiny_skia::SpreadMode::Pad
                    } else {
                        tiny_skia::SpreadMode::Repeat
                    },
                    FilterQuality::Bicubic,
                    alpha,
                    transform.into_skia(),
                );
                Ok(Cow::Owned(r))
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
        p: &SkiaPath,
        current: Option<MaskEntry>,
        rule: FillRule,
        create_mask: impl FnOnce() -> Result<Mask>,
    ) -> Result<MaskEntry> {
        debug_assert!(!p.is_empty());

        let (new_path, cur_mask) = match current {
            None => (p.clone(), None),
            Some(cur) => {
                let mut r = PathBuilder::new();
                r.push_path(&cur.0);
                r.push_path(p);
                (
                    r.finish().whatever_context("get path")?,
                    Some(Rc::clone(&cur.1)),
                )
            }
        };

        for (i, e) in self.recents.iter().enumerate() {
            if e.0.as_ref() == &new_path {
                let e = e.clone();
                self.recents.swap(0, i);
                return Ok(e);
            }
        }

        let mut mask: Mask = cur_mask.map_or_else(create_mask, |m| Ok(m.borrow().clone()))?;
        mask.intersect_path(p, rule, true, Transform::identity());
        let entry = (Rc::new(new_path), Rc::new(RefCell::new(mask)));
        if self.recents.len() == N {
            self.recents.pop_back();
        }
        self.recents.push_front(entry.clone());
        Ok(entry)
    }
}

#[derive(Debug, Clone, Educe)]
#[educe(Default)]
struct ColorState {
    // apply before `self.paint` if not null
    background_paint: Option<PaintCreator>,

    #[educe(Default(expression = PaintCreator::Color(SkiaColor::BLACK)))]
    paint: PaintCreator,
    #[educe(Default(expression = ColorSpace::DeviceGray))]
    color_space: ColorSpace<f32>,
    #[educe(Default = 1.0f32)]
    alpha: f32,
    alpha_is_shape: bool,
}

impl ColorState {
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }

    /// Set color space, if args is None, set color to color space default color
    pub fn set_color_space(
        &mut self,
        cs: ColorSpace<f32>,
        args: Option<impl AsRef<[f32]>>,
    ) -> Result<()> {
        self.color_space = cs;
        if let Some(args) = args {
            self.set_color_args(args)?;
        } else {
            let [r, g, b, a] = self.color_space.default_color();
            self.set_paint(
                PaintCreator::Color(
                    SkiaColor::from_rgba(r, g, b, a).whatever_context("Convert color from rgba")?,
                ),
                None,
            );
        }
        Ok(())
    }

    pub fn set_color_args(&mut self, color_args: impl AsRef<[f32]>) -> Result<()> {
        let color = to_skia_color(&self.color_space, color_args.as_ref())?;
        self.set_paint(PaintCreator::Color(color), None);
        Ok(())
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
    ) -> Result<()> {
        if let Some(paint) = &self.background_paint {
            canvas.stroke_path(
                path,
                paint.create(self.alpha())?.as_ref(),
                stroke,
                transform,
                mask,
            );
        }
        canvas.stroke_path(
            path,
            self.paint.create(self.alpha())?.as_ref(),
            stroke,
            transform,
            mask,
        );
        Ok(())
    }

    pub fn create_paint(&self) -> Result<Cow<'_, Paint<'_>>> {
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
    ) -> Result<()> {
        if let Some(paint) = &self.background_paint {
            canvas.fill_path(
                path,
                paint.create(self.alpha())?.as_ref(),
                fill_rule,
                transform,
                mask,
            );
        }
        canvas.fill_path(
            path,
            self.paint.create(self.alpha())?.as_ref(),
            fill_rule,
            transform,
            mask,
        );
        Ok(())
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
    /// If not None, update mask with path on end_path
    clipping: Option<FillRule>,
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
            clipping: None,
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

    #[allow(clippy::needless_pass_by_ref_mut)]
    #[allow(clippy::unused_self)]
    fn set_flatness(&mut self, flatness: f32) {
        info!("not implemented: flatness: {}", flatness);
    }

    #[allow(clippy::needless_pass_by_ref_mut)]
    #[allow(clippy::unused_self)]
    fn set_render_intent(&mut self, intent: RenderingIntent) {
        info!("not implemented: render intent: {}", intent);
    }

    fn get_fill_paint(&self) -> Result<Cow<'_, Paint<'_>>> {
        self.fill_state.create_paint()
    }

    fn get_stroke_paint(&self) -> Result<Cow<'_, Paint<'_>>> {
        self.stroke_state.create_paint()
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> ImageToDeviceSpace {
        image_to_user_space(img_w, img_h).then(&self.user_to_device)
    }

    fn get_mask(&self) -> Option<Ref<'_, Mask>> {
        self.mask.as_ref().map(|m| m.1.borrow())
    }

    fn set_graphics_state(&mut self, res: &GraphicsStateParameterDict<'_, '_>) -> Result<()> {
        for key in res.dict().keys() {
            match key.as_str() {
                "LW" => self.set_line_width(
                    res.line_width()
                        .whatever_context("get line width")?
                        .whatever_context("unwrap line width")?,
                ),
                "LC" => self.set_line_cap(
                    res.line_cap()
                        .whatever_context("get line cap")?
                        .whatever_context("unwrap line cap")?,
                ),
                "LJ" => self.set_line_join(
                    res.line_join()
                        .whatever_context("get line join")?
                        .whatever_context("unwrap line join")?,
                ),
                "ML" => self.set_miter_limit(
                    res.miter_limit()
                        .whatever_context("get miter limit")?
                        .whatever_context("unwrap miter limit")?,
                ),
                "RI" => self.set_render_intent(
                    res.rendering_intent()
                        .whatever_context("get rendering intent")?
                        .whatever_context("unwrap rendering intent")?,
                ),
                "TK" => self.set_text_knockout_flag(
                    res.text_knockout_flag()
                        .whatever_context("get text knockout flag")?
                        .whatever_context("unwrap text knockout flag")?,
                )?,
                "FL" => self.set_flatness(
                    res.flatness()
                        .whatever_context("get flatness")?
                        .whatever_context("unwrap flatness")?,
                ),
                "CA" => self.set_stroke_alpha(
                    res.stroke_alpha()
                        .whatever_context("get stroke alpha")?
                        .whatever_context("unwrap stroke alpha")?,
                ),
                "ca" => self.set_fill_alpha(
                    res.fill_alpha()
                        .whatever_context("get fill alpha")?
                        .whatever_context("unwrap fill alpha")?,
                ),
                "AIS" => self.set_alpha_is_shape(
                    res.alpha_is_shape()
                        .whatever_context("get alpha is shape")?
                        .whatever_context("unwrap alpha is shape")?,
                ),
                "Type" => (),
                "SM" => debug!("ExtGState key: SM (smoothness tolerance) not implemented"),
                k @ ("OPM" | "op" | "OP") => {
                    debug!("ExtGState key {k} is for Overprint, which is not supported");
                }
                "SA" => {
                    debug!(
                        "Unknown or unsupported ExtGState key: SA (automatic stroke adjustment)"
                    );
                }
                _ => info!("Unknown or unsupported ExtGState key: {}", key.as_ref()),
            }
        }
        Ok(())
    }

    fn update_mask(
        &mut self,
        path: impl CloneOrMove<Target = SkiaPath>,
        rule: FillRule,
        flip_y: bool,
    ) -> Result<()> {
        let w = self.dimension.canvas_width();
        let h = self.dimension.canvas_height();
        let new_mask = || {
            let mut r = Mask::new(w, h).whatever_context("create new mask")?;
            let p = PathBuilder::from_rect(
                Rect::from_xywh(0.0, 0.0, w as f32, h as f32).whatever_context("create rect")?,
            );
            r.fill_path(&p, FillRule::Winding, true, Transform::identity());
            Ok(r)
        };

        let mut path = path.clone_or_move();
        if flip_y {
            path = path
                .transform(self.user_to_device.into_skia())
                .whatever_context("transform path")?;
        }

        self.mask = Some(self.mask_cache.borrow_mut().update(
            &path,
            self.mask.clone(),
            rule,
            new_mask,
        )?);
        Ok(())
    }

    fn set_text_knockout_flag(&mut self, knockout: bool) -> Result<()> {
        self.text_object.knockout = knockout;
        // default value of knockout is true, so set to true don't change anything
        if !knockout {
            error!("TODO: impl text knockout");
        }
        Ok(())
    }

    pub fn end_text_object(&mut self) -> Result<()> {
        // if exists text clipping path, intersection to current clipping path using Winding fill
        // rule
        let p = self.text_object.text_clipping_path.finish()?;
        if let Some(p) = p {
            let p = p.to_owned();
            self.update_mask(p, FillRule::Winding, false)?;
            self.text_object.text_clipping_path.reset();
        }
        Ok(())
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
    #[educe(Default(expression = Left(PathBuilder::new())))]
    path: Either<PathBuilder, SkiaPath>,
}

impl Path {
    fn path_builder(&mut self) -> Result<&mut PathBuilder> {
        self.path
            .as_mut()
            .left()
            .whatever_context("get path builder")
    }

    pub fn close_path(&mut self) -> Result<()> {
        self.path_builder()?.close();
        Ok(())
    }

    pub fn move_to(&mut self, p: Point) -> Result<()> {
        self.path_builder()?.move_to(p.x, p.y);
        Ok(())
    }

    pub fn line_to(&mut self, p: Point) -> Result<()> {
        self.path_builder()?.line_to(p.x, p.y);
        Ok(())
    }

    pub fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) -> Result<()> {
        self.path_builder()?
            .cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
        Ok(())
    }

    pub fn curve_to_cur_point_as_control(&mut self, p2: Point, p3: Point) -> Result<()> {
        let p1 = self
            .path_builder()?
            .last_point()
            .whatever_context("get path last point")?;
        self.curve_to(Point::new(p1.x, p1.y), p2, p3)
    }

    pub fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) -> Result<()> {
        self.curve_to(p1, p3, p3)
    }

    pub fn append_rect(&mut self, p: Point, w: f32, h: f32) -> Result<()> {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path_builder()?.push_rect(r.into_skia()?);
        Ok(())
    }

    /// Build path and clear the path builder, return None if path is empty
    pub fn finish(&mut self) -> Result<Option<&SkiaPath>> {
        if let Left(_) = self.path {
            let temp = Left(PathBuilder::new());
            let pb = std::mem::replace(&mut self.path, temp)
                .left()
                .whatever_context("get last PathBuilder")?;
            if pb.is_empty() {
                return Ok(None);
            }

            if let Some(p) = pb.finish() {
                self.path = Right(p);
            } else {
                debug!("invalid path");
            }
        }

        match &self.path {
            Left(_) => Ok(None),
            Right(p) => Ok(Some(p)),
        }
    }

    pub fn reset(&mut self) {
        let temp = Left(PathBuilder::new());
        let p = std::mem::replace(&mut self.path, temp);
        self.path = p
            .map_left(|mut p| {
                p.clear();
                p
            })
            .right_and_then(|p| Left(p.clear()));
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
pub struct Render<'a, 'c> {
    nested_level: u16,
    canvas: &'c mut Pixmap,
    stack: Vec<State>,
    path: Path,
    #[educe(Debug(ignore))]
    font_cache: FontCache<'c, SkiaPathSink>,
    resources: &'c ResourceDict<'a, 'a>,
    dimension: PageDimension,
}

impl<'a, 'c> Render<'a, 'c> {
    fn create(
        nested_level: u16,
        canvas: &'c mut Pixmap,
        option: RenderOption,
        resources: &'c ResourceDict<'a, 'a>,
    ) -> Result<Self>
    where
        'a: 'c,
    {
        let mut state = if let Some(state) = option.state {
            state
        } else {
            State::new(&option)
        };

        if let Some(rect) = option.crop {
            state.update_mask(
                PathBuilder::from_rect(rect.into_skia()?),
                FillRule::Winding,
                true,
            )?;
        }

        Ok(Self {
            nested_level,
            canvas,
            stack: vec![state],
            path: Path::default(),
            font_cache: FontCache::new(resources).whatever_context("Create font cache")?,
            resources,
            dimension: option.dimension,
        })
    }

    /// Return None if nested level is greater than 10, to avoid infinite loop
    fn new_nested(
        cur_level: u16,
        canvas: &'c mut Pixmap,
        option: RenderOption,
        resources: &'c ResourceDict<'a, 'a>,
    ) -> Result<Option<Self>> {
        Ok(if cur_level < 10 {
            Some(Self::create(cur_level + 1, canvas, option, resources)?)
        } else {
            warn!("nested level is greater than 10");
            None
        })
    }

    pub fn new(
        canvas: &'c mut Pixmap,
        option: RenderOption,
        resources: &'c ResourceDict<'a, 'a>,
    ) -> Result<Self>
    where
        'a: 'c,
    {
        Self::create(0, canvas, option, resources)
    }

    fn device_width(&self) -> u32 {
        self.canvas.width()
    }

    fn device_height(&self) -> u32 {
        self.canvas.height()
    }

    fn push(&mut self) -> Result<()> {
        self.stack.push(Self::top(&self.stack)?.clone());
        Ok(())
    }

    fn top(stack: &[State]) -> Result<&State> {
        stack.last().whatever_context("get stack top")
    }

    fn pop(&mut self) {
        if self.stack.len() <= 1 {
            // some file contains unpaired q/Q operations
            warn!("cannot pop state stack with only one state remaining");
        } else {
            self.stack.pop();
        }
    }

    fn current_mut(stack: &mut Vec<State>) -> Result<&mut State> {
        stack.last_mut().whatever_context("get current state")
    }

    fn text_object(&self) -> Result<&TextObject> {
        Ok(&Self::top(&self.stack)?.text_object)
    }

    fn text_object_mut(&mut self) -> Result<&mut TextObject> {
        Ok(&mut Self::current_mut(&mut self.stack)?.text_object)
    }

    pub(crate) fn exec(&mut self, op: Operation) -> Result<()> {
        let start = std::time::Instant::now();
        let result = self._exec(op.clone());
        let duration = start.elapsed();
        if duration > std::time::Duration::from_millis(15) {
            warn!("Operation {:?} took {:?} to execute", op, duration);
        }
        result
    }

    fn _exec(&mut self, op: Operation) -> Result<()> {
        debug!("handle operation: {:?}", op);
        match op {
            // General Graphics State Operations
            Operation::SetLineWidth(width) => {
                Self::current_mut(&mut self.stack)?.set_line_width(width)
            }
            Operation::SetLineCap(cap) => Self::current_mut(&mut self.stack)?.set_line_cap(cap),
            Operation::SetLineJoin(join) => Self::current_mut(&mut self.stack)?.set_line_join(join),
            Operation::SetMiterLimit(limit) => {
                Self::current_mut(&mut self.stack)?.set_miter_limit(limit)
            }
            Operation::SetDashPattern(pattern, phase) => {
                Self::current_mut(&mut self.stack)?.set_dash_pattern(&pattern, phase);
            }
            Operation::SetRenderIntent(intent) => {
                Self::current_mut(&mut self.stack)?.set_render_intent(intent)
            }
            Operation::SetFlatness(flatness) => {
                Self::current_mut(&mut self.stack)?.set_flatness(flatness)
            }
            Operation::SetGraphicsStateParameters(nm) => {
                let res = self
                    .resources
                    .ext_g_state()
                    .whatever_context("get page resources")?;
                if let Some(res) = res.get(&nm.0) {
                    Self::current_mut(&mut self.stack)?.set_graphics_state(res)?;
                } else {
                    warn!("ExtGState not found {}", nm.0);
                }
            }

            // Special Graphics State Operations
            Operation::SaveGraphicsState => self.push()?,
            Operation::RestoreGraphicsState => self.pop(),
            Operation::ModifyCTM(ctm) => Self::current_mut(&mut self.stack)?.concat_ctm(ctm),

            // Path Construction Operations
            Operation::MoveToNext(p) => self.path.move_to(p)?,
            Operation::LineToNext(p) => self.path.line_to(p)?,
            Operation::AppendBezierCurve(p1, p2, p3) => self.path.curve_to(p1, p2, p3)?,
            Operation::AppendBezierCurve2(p2, p3) => {
                self.path.curve_to_cur_point_as_control(p2, p3)?;
            }
            Operation::AppendBezierCurve1(p1, p3) => {
                self.path.curve_to_dest_point_as_control(p1, p3)?;
            }
            Operation::ClosePath => self.path.close_path()?,
            Operation::AppendRectangle(p, w, h) => self.path.append_rect(p, w, h)?,

            // Path Painting Operation
            Operation::Stroke => self.stroke()?,
            Operation::CloseAndStroke => self.close_and_stroke()?,
            Operation::FillNonZero | Operation::FillNonZeroDeprecated => {
                self.fill_path_non_zero()?;
            }
            Operation::FillEvenOdd => self.fill_path_even_odd()?,
            Operation::FillAndStrokeNonZero => self.fill_and_stroke_non_zero()?,
            Operation::FillAndStrokeEvenOdd => self.fill_and_stroke_even_odd()?,
            Operation::CloseFillAndStrokeNonZero => self.close_fill_and_stroke_non_zero()?,
            Operation::CloseFillAndStrokeEvenOdd => self.close_fill_and_stroke_even_odd()?,
            Operation::EndPath => self.end_path()?,

            // Clipping Path Operations
            Operation::ClipNonZero => {
                Self::current_mut(&mut self.stack)?.clipping = Some(FillRule::Winding);
            }
            Operation::ClipEvenOdd => {
                Self::current_mut(&mut self.stack)?.clipping = Some(FillRule::EvenOdd);
            }

            // Text Object Operations
            Operation::BeginText => self.text_object_mut()?.reset(),
            Operation::EndText => self.end_text()?,

            // Text State Operations
            Operation::SetCharacterSpacing(spacing) => {
                self.text_object_mut()?.set_character_spacing(spacing);
            }
            Operation::SetWordSpacing(spacing) => self.text_object_mut()?.set_word_spacing(spacing),
            Operation::SetHorizontalScaling(scale) => {
                self.text_object_mut()?.set_horizontal_scaling(scale);
            }
            Operation::SetLeading(leading) => self.text_object_mut()?.set_leading(leading),
            Operation::SetFont(name, size) => self.text_object_mut()?.set_font(name, size),
            Operation::SetTextRenderingMode(mode) => {
                self.text_object_mut()?.set_text_rendering_mode(mode);
            }
            Operation::SetTextRise(rise) => self.text_object_mut()?.set_text_rise(rise)?,

            // Text Positioning Operations
            Operation::MoveTextPosition(p) => self.text_object_mut()?.move_text_position(p),
            Operation::MoveTextPositionAndSetLeading(p) => {
                self.text_object_mut()?.set_leading(-p.y);
                self.text_object_mut()?.move_text_position(p);
            }
            Operation::SetTextMatrix(m) => self.text_object_mut()?.set_text_matrix(m),
            Operation::MoveToStartOfNextLine => self.move_to_start_of_next_line()?,

            // Text Showing Operations
            Operation::ShowText(text) => self.show_text(text.to_bytes())?,
            Operation::MoveToNextLineAndShowText(text) => {
                self.move_to_start_of_next_line()?;
                self.show_text(text.to_bytes())?;
            }
            Operation::ShowTexts(texts) => self.show_texts(&texts)?,
            Operation::SetSpacingMoveToNextLineAndShowText(aw, ac, text) => {
                self.text_object_mut()?.set_word_spacing(aw);
                self.text_object_mut()?.set_character_spacing(ac);
                self.move_to_start_of_next_line()?;
                self.show_text(text.to_bytes())?;
            }

            // Color Operations
            Operation::SetStrokeColorSpace(args) => {
                let cs =
                    ColorSpace::from_args(&args, self.resources.resolver(), Some(self.resources))
                        .whatever_context("Create ColorSpace")?;
                self.set_color_and_space(Self::stroke_color_state, cs, None)?;
            }
            Operation::SetFillColorSpace(args) => {
                let cs =
                    ColorSpace::from_args(&args, self.resources.resolver(), Some(self.resources))
                        .whatever_context("Create ColorSpace")?;
                self.set_color_and_space(Self::fill_color_state, cs, None)?;
            }
            Operation::SetStrokeColor(args) => {
                self.set_color_args(Self::stroke_color_state, &args)?;
            }
            Operation::SetStrokeGray(color) => self.set_color_and_space(
                Self::stroke_color_state,
                ColorSpace::DeviceGray,
                Some(&color),
            )?,
            Operation::SetStrokeCMYK(color) => self.set_color_and_space(
                Self::stroke_color_state,
                ColorSpace::DeviceCMYK,
                Some(&color),
            )?,
            Operation::SetStrokeRGB(color) => self.set_color_and_space(
                Self::stroke_color_state,
                ColorSpace::DeviceRGB,
                Some(&color),
            )?,
            Operation::SetStrokeColorOrWithPattern(color_or_name) => {
                self.set_color_or_pattern(Self::stroke_color_state, &color_or_name)?;
            }
            Operation::SetFillColor(args) => self.set_color_args(Self::fill_color_state, &args)?,
            Operation::SetFillGray(color) => self.set_color_and_space(
                Self::fill_color_state,
                ColorSpace::DeviceGray,
                Some(&color),
            )?,
            Operation::SetFillCMYK(color) => self.set_color_and_space(
                Self::fill_color_state,
                ColorSpace::DeviceCMYK,
                Some(&color),
            )?,
            Operation::SetFillRGB(color) => self.set_color_and_space(
                Self::fill_color_state,
                ColorSpace::DeviceRGB,
                Some(&color),
            )?,
            Operation::SetFillColorOrWithPattern(color_or_name) => {
                self.set_color_or_pattern(Self::fill_color_state, &color_or_name)?;
            }

            // Shading Operation
            Operation::PaintShading(name) => self.paint_shading(&name)?,

            // XObject Operation
            Operation::PaintXObject(name) => self.paint_x_object(&name)?,

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
            Operation::SetGlyphWidth(_) | Operation::SetGlyphWidthAndBoundingBox(_, _, _) => {}

            Operation::PaintInlineImage(inline_image) => {
                self.paint_inline_image(&inline_image)?;
            }

            _ => whatever!("unimplemented operation: {:?}", op),
        }
        Ok(())
    }

    fn move_to_start_of_next_line(&mut self) -> Result<()> {
        let leading = Self::top(&self.stack)?.text_object.leading;
        self.text_object_mut()?
            .move_text_position(TextPoint::new(0.0, -leading));
        Ok(())
    }

    fn set_color_args(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> Result<&mut ColorState>,
        args: &ColorArgs,
    ) -> Result<()> {
        let state = get_state(self)?;
        state.set_color_args(args)?;
        Ok(())
    }

    fn set_color_and_space(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> Result<&mut ColorState>,
        cs: ColorSpace<f32>,
        color: Option<&[f32]>,
    ) -> Result<()> {
        let state = get_state(self)?;
        state.set_color_space(cs, color)
    }

    fn stroke(&mut self) -> Result<()> {
        if let Some(p) = self.path.finish()? {
            let state = Self::top(&self.stack)?;
            let stroke = state.get_stroke();
            state.stroke_state.stroke(
                self.canvas,
                p,
                stroke,
                state.user_to_device.into_skia(),
                state.get_mask().as_deref(),
            )?;
        } else {
            debug!("stroke: empty or invalid path");
        }
        self.end_path()
    }

    fn end_path(&mut self) -> Result<()> {
        let state = Self::current_mut(&mut self.stack)?;
        if let Some(rule) = state.clipping {
            if let Some(p) = self.path.finish()? {
                state.update_mask(p, rule, true)?;
            }
            state.clipping = None;
        }
        self.path.reset();
        Ok(())
    }

    fn close_path(&mut self) -> Result<()> {
        self.path.close_path()
    }

    fn close_and_stroke(&mut self) -> Result<()> {
        self.close_path()?;
        self.stroke()
    }

    fn _fill(&mut self, fill_rule: FillRule, reset_path: bool) -> Result<()> {
        let state = Self::top(&self.stack)?;
        if let Some(p) = self.path.finish()? {
            state.fill_state.fill(
                self.canvas,
                p,
                fill_rule,
                state.user_to_device.into_skia(),
                state.get_mask().as_deref(),
            )?;
        }
        if reset_path {
            self.end_path()?;
        }
        Ok(())
    }

    fn fill_path_non_zero(&mut self) -> Result<()> {
        self._fill(FillRule::Winding, true)
    }

    fn fill_path_even_odd(&mut self) -> Result<()> {
        self._fill(FillRule::EvenOdd, true)
    }

    fn fill_and_stroke_non_zero(&mut self) -> Result<()> {
        self._fill(FillRule::Winding, false)?;
        self.stroke()
    }

    fn fill_and_stroke_even_odd(&mut self) -> Result<()> {
        self._fill(FillRule::EvenOdd, false)?;
        self.stroke()
    }

    fn close_fill_and_stroke_non_zero(&mut self) -> Result<()> {
        self.close_path()?;
        self.fill_and_stroke_non_zero()
    }

    fn close_fill_and_stroke_even_odd(&mut self) -> Result<()> {
        self.close_path()?;
        self.fill_and_stroke_even_odd()
    }

    fn load_image_as_mask(mut img: RgbaImage, state: &State, s_mask: bool) -> Result<Mask> {
        let paint = PixmapPaint {
            quality: FilterQuality::Nearest,
            ..Default::default()
        };

        let mut canvas = Pixmap::new(
            state.dimension.canvas_width(),
            state.dimension.canvas_height(),
        )
        .whatever_context("Create image for creating mask")?;
        img.pixels_mut()
            .for_each(|p| p[3] = if s_mask { p[0] } else { !p[0] });

        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height())
            .whatever_context("Create mask image")?;
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

    fn paint_inline_image(&mut self, inline_image: &InlineImage) -> Result<()> {
        let state = Self::top(&self.stack)?;
        let meta = inline_image.meta();
        let img = inline_image
            .image(self.resources.resolver(), self.resources)
            .whatever_context("decode image")?
            .into_rgba8();

        if meta.image_mask().whatever_context("get image mask")? {
            let domain = meta
                .decode()
                .whatever_context("decode domain")?
                .map_or_else(|| Domain::new(0.0, 1.0), |domains| domains[0]);
            let mask_reversed = domain.start > domain.end;
            let mask = Self::load_image_as_mask(img, state, mask_reversed)?;
            // fill canvas with current fill paint with mask
            let paint = state.get_fill_paint()?;
            self.canvas.fill_rect(
                Rect::from_xywh(
                    0.0,
                    0.0,
                    self.device_width() as f32,
                    self.device_height() as f32,
                )
                .whatever_context("create inline image rect")?,
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
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height())
            .whatever_context("Create inline image")?;
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

    fn paint_image_x_object(&mut self, x_object: &XObjectDict<'a, '_>) -> Result<()> {
        fn load_image<'a, 'b>(
            image_dict: &XObjectDict<'a, 'b>,
            resources: &ResourceDict<'a, 'b>,
        ) -> Result<RgbaImage> {
            let image = image_dict
                .as_stream()
                .whatever_context("Only Image XObject supported")?;
            Ok(image
                .decode_image(resources.resolver(), Some(resources))
                .with_whatever_context(|_| format!("decode image {:?}", image.id()))?
                .into_rgba8())
        }

        let state = Self::top(&self.stack)?;

        if x_object.image_mask().whatever_context("get image mask")? {
            let is_invert =
                if let Some(decode) = x_object.decode().whatever_context("decode x_object")? {
                    let domain = decode.0[0];
                    domain.start > domain.end
                } else {
                    false
                };
            let x_object = x_object
                .as_stream()
                .whatever_context("get x_object stream")?;
            let img = x_object
                .decode_image(self.resources.resolver(), Some(self.resources))
                .with_whatever_context(|_| {
                    format!("decode x_object to image, id: {:?}", x_object.id())
                })?;
            let mask = Self::load_image_as_mask(img.into_rgba8(), state, is_invert)?;
            // fill canvas with current fill paint with mask
            let paint = state.get_fill_paint()?;
            self.canvas.fill_rect(
                Rect::from_xywh(
                    0.0,
                    0.0,
                    self.device_width() as f32,
                    self.device_height() as f32,
                )
                .whatever_context("Create x_object image rect")?,
                &paint,
                Transform::identity(),
                Some(&mask),
            );
            return Ok(());
        }

        let s_mask = x_object
            .s_mask()
            .whatever_context("read x_object s_mask")?
            .map(|s_mask| {
                let s_mask_stream = s_mask.as_stream().whatever_context("get s_mask stream")?;
                let img = s_mask_stream
                    .decode_image(self.resources.resolver(), Some(self.resources))
                    .with_whatever_context(|_| format!("decode s_mask image, {:?}", s_mask.id()))?;
                Self::load_image_as_mask(img.into_rgba8(), state, true)
            })
            .or_else(|| {
                let mask = match x_object.mask() {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(Err(AnyWhatever::with_source(
                            Box::new(e),
                            "get x_object mask".to_owned(),
                        )));
                    }
                };
                let Some(ImageMask::Explicit(mask)) = mask else {
                    return None;
                };
                let img = match mask
                    .decode_image(self.resources.resolver(), Some(self.resources))
                    .with_whatever_context(|_| format!("decode mask image: {:?}", mask.id()))
                {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                Some(Self::load_image_as_mask(img.into_rgba8(), state, false))
            })
            .transpose()?;

        let paint = PixmapPaint {
            opacity: state.fill_state.alpha(),
            quality: if x_object
                .interpolate()
                .whatever_context("read x_object interpolate")?
            {
                FilterQuality::Bilinear
            } else {
                FilterQuality::Nearest
            },
            ..Default::default()
        };
        let img = load_image(x_object, self.resources)?;
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height())
            .whatever_context("Create x_object image")?;
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
    /// 1. Assert form b_box start point is (0, 0), because I'm not sure what will happen, wait for
    ///    an example pdf file that b_box start point is not (0, 0)
    /// 1. Paints the graphics objects specified in the form object's stream in sub render.
    /// 1. Paint the rendered image on parent render
    fn paint_form_x_object(&mut self, x_object: &XObjectDict<'a, 'a>) -> Result<()> {
        let form = x_object
            .as_form()
            .whatever_context("read x_object as form")?;
        let matrix = form.matrix().whatever_context("read form matrix")?;
        let b_box = form.b_box().whatever_context("read form b_box")?;
        let stream = x_object
            .as_stream()
            .whatever_context("read x_object stream")?;
        let stream = stream
            .decode(self.resources.resolver())
            .with_whatever_context(|_| {
                format!("decode x_object to image, id: {:?}", stream.id())
            })?;
        let content = PageContent::new(vec![stream.into_owned()]);
        let resources = form.resources().whatever_context("get form resources")?;
        let resources = resources.as_ref().unwrap_or(self.resources);

        let state = Self::top(&self.stack)?;
        let mut inner_state = state.clone();
        let ctm = matrix.then(&state.ctm).with_destination().with_source();
        inner_state.set_ctm(ctm);
        let Some(mut render) = Render::new_nested(
            self.nested_level,
            self.canvas,
            RenderOptionBuilder::default()
                .dimension(self.dimension)
                .crop(Some(b_box))
                .background_color(SkiaColor::TRANSPARENT)
                .state(inner_state)
                .build(),
            resources,
        )?
        else {
            return Ok(());
        };
        content
            .operations()
            .whatever_context("get form page operations")?
            .into_iter()
            .try_for_each(|op| render.exec(op))?;

        Ok(())
    }

    /// Paints the specified XObject. Only XObjectType::Image supported
    fn paint_x_object(&mut self, nm: &NameOfDict) -> Result<()> {
        let x_objects = self
            .resources
            .x_object()
            .whatever_context("read resources x_object")?;
        let x_object = x_objects.get(&nm.0);

        if let Some(x_object) = x_object {
            if x_object.as_stream().is_ok() {
                match x_object
                    .subtype()
                    .whatever_context("get x_object subtype")?
                {
                    XObjectType::Image => self.paint_image_x_object(x_object),
                    XObjectType::Form => self.paint_form_x_object(x_object),
                    t => whatever!("TODO: {:?}", t),
                }
            } else {
                warn!("x_object {} not stream, ignored", nm.0);
                return Ok(());
            }
        } else {
            warn!("x_object {} not found", nm.0);
            Ok(())
        }
    }

    fn paint_axial(&mut self, axial: &Axial) -> Result<()> {
        let b_box = axial.b_box;

        let state = Self::top(&self.stack)?;
        let ctm = state.user_to_device.into_skia();
        let (shader_ctm, fill_ctm, path) = if let Some(b_box) = b_box {
            (
                Transform::identity(),
                ctm,
                PathBuilder::from_rect(b_box.into_skia()?),
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
                    .into_skia()?,
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

    fn paint_radial(&mut self, radial: &Radial) -> Result<()> {
        let Domain { start: t0, end: t1 } = radial.domain;
        let (x0, y0) = (radial.start.point.x, radial.start.point.y);
        let (x1, y1) = (radial.end.point.x, radial.end.point.y);
        let r0 = radial.start.r;
        let r1 = radial.end.r;
        let state = Self::top(&self.stack)?;
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
        .ceil()
        .to_usize()
        .whatever_context("convert f32 steps to usize")?;
        let steps = steps.max(10);

        let ctm = ctm.into_skia();
        if radial.extend.end() {
            let c = radial
                .function
                .call(&[1.0])
                .whatever_context("exec function")?;
            let mut c = radial
                .color_space
                .to_rgba(c.as_slice())
                .whatever_context("convert to rgba")?;
            c[3] = state.fill_state.alpha();
            paint.set_color(
                SkiaColor::from_rgba(c[0], c[1], c[2], c[3])
                    .whatever_context("convert from rgba")?,
            );
            self.canvas.fill_rect(
                Rect::from_xywh(
                    0.0,
                    0.0,
                    self.device_width() as f32,
                    self.device_height() as f32,
                )
                .whatever_context("create rect")?,
                &paint,
                Transform::identity(),
                state.get_mask().as_deref(),
            );
        }

        if radial.extend.begin() && radial.start.r > 0.0 {
            let (x, y) = (radial.start.point.x, radial.start.point.y);
            let r = radial.start.r;
            let c = radial
                .function
                .call(&[0.0])
                .whatever_context("exec function")?;
            let c = radial
                .color_space
                .to_rgba(c.as_slice())
                .whatever_context("to rgba")?;
            paint.set_color(
                SkiaColor::from_rgba(c[0], c[1], c[2], c[3]).whatever_context("from rgba")?,
            );
            let path = PathBuilder::from_circle(x, y, r).whatever_context("create circle")?;
            let path = path
                .transform(ctm)
                .whatever_context("ctm transform circle")?;
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
            let c = radial
                .function
                .call(&[t][..])
                .whatever_context("exec function")?;
            let c = radial
                .color_space
                .to_rgba(c.as_slice())
                .whatever_context("to rgba")?;

            let Some(path) = PathBuilder::from_circle(x, y, r) else {
                continue;
            };
            let path = path.transform(ctm).whatever_context("path ctm transform")?;
            paint.set_color(
                SkiaColor::from_rgba(c[0], c[1], c[2], c[3])
                    .whatever_context("skia color from rgba")?,
            );
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

    fn paint_shading(&mut self, nm: &NameOfDict) -> Result<()> {
        let shading = self
            .resources
            .shading()
            .whatever_context("get shading resource")?;
        let Some(shading) = shading.get(&nm.0) else {
            return Ok(warn!("shading {} not found", nm.0));
        };
        match build_shading(shading, self.resources).whatever_context("build shading")? {
            Some(Shading::Radial(radial)) => {
                self.paint_radial(&radial).whatever_context("paint radial")
            }
            Some(Shading::Axial(axial)) => self.paint_axial(&axial).whatever_context("paint axial"),
            None => Ok(()),
        }
    }

    fn fill_color_state(&mut self) -> Result<&mut ColorState> {
        Ok(&mut Self::current_mut(&mut self.stack)?.fill_state)
    }

    fn stroke_color_state(&mut self) -> Result<&mut ColorState> {
        Ok(&mut Self::current_mut(&mut self.stack)?.stroke_state)
    }

    fn set_color_or_pattern(
        &mut self,
        mut get_state: impl FnMut(&mut Self) -> Result<&mut ColorState>,
        color_or_name: &ColorArgsOrName,
    ) -> Result<()> {
        match color_or_name {
            ColorArgsOrName::Name((name, color_args)) => {
                let pattern = self.resources.pattern().whatever_context("get pattern")?;
                let pattern = &pattern[name];
                match pattern
                    .pattern_type()
                    .whatever_context("get pattern type")?
                {
                    PatternType::Tiling => {
                        let dimension = Size2D::new(
                            self.dimension.canvas_width() as f32,
                            self.dimension.canvas_height() as f32,
                        );
                        self.tiling_pattern(
                            dimension,
                            get_state,
                            &pattern
                                .tiling_pattern()
                                .whatever_context("get tiling pattern")?,
                            color_args.as_ref(),
                        )
                    }
                    PatternType::Shading => {
                        if let Some((paint, background_color)) = self.shading_pattern(
                            &pattern
                                .shading_pattern()
                                .whatever_context("get shading pattern")?,
                        )? {
                            let color_state = get_state(self)?;
                            color_state.set_paint(paint, background_color);
                        }
                        Ok(())
                    }
                }
            }
            ColorArgsOrName::Color(args) => {
                let state = get_state(self)?;
                state.set_color_args(args)?;
                Ok(())
            }
        }
    }

    fn shading_pattern(
        &mut self,
        pattern: &ShadingPatternDict<'a, 'a>,
    ) -> Result<Option<(PaintCreator, Option<SkiaColor>)>> {
        struct RestoreState<F>(Option<F>)
        where
            F: FnOnce();
        impl<F> Drop for RestoreState<F>
        where
            F: FnOnce(),
        {
            fn drop(&mut self) {
                if let Some(f) = self.0.take() {
                    f();
                }
            }
        }

        let resources = self.resources;
        let _restore = if let Some(ext_g_state) =
            pattern.ext_g_state().whatever_context("read ext_g_state")?
        {
            self.push()?;
            Render::current_mut(&mut self.stack)?.set_graphics_state(&ext_g_state)?;
            Some(RestoreState(Some(|| self.pop())))
        } else {
            None
        };

        let shading = pattern.shading().whatever_context("get shading")?;
        // assert!(shading.b_box()?.is_none(), "TODO: support BBox of shading");
        let background_color =
            (if let Some(args) = shading.background().whatever_context("get background")? {
                let cs = shading.color_space().whatever_context("get color space")?;
                let cs = ColorSpace::from_args(&cs, resources.resolver(), Some(resources))
                    .whatever_context("Create ColorSpace")?;
                Some(to_skia_color(&cs, args.as_ref()))
            } else {
                None
            })
            .transpose()?;

        Ok(
            match build_shading(&shading, resources).whatever_context("build shading pattern")? {
                Some(shading) => Some((
                    shading,
                    pattern.matrix().whatever_context("get pattern matrix")?,
                )),
                None => return Ok(None),
            }
            .map(|shader| (PaintCreator::Gradient(shader), background_color)),
        )
    }

    fn tiling_pattern(
        &mut self,
        canvas_size: Size2D<f32>,
        mut get_state: impl FnMut(&mut Self) -> Result<&mut ColorState>,
        tile: &TilingPatternDict<'a, 'a>,
        color_args: Option<&ColorArgs>,
    ) -> Result<()> {
        let stream: &Object = tile
            .resolver()
            .resolve(tile.id())
            .whatever_context("resolve tile object")?;
        let stream = stream.as_stream().whatever_context("get tile stream")?;
        let bytes = stream
            .decode(tile.resolver())
            .whatever_context("decode tile stream")?;
        let ops = terminated(parse_operations::<ParserError>, rest)
            .parse(bytes.as_ref())
            .map_err(winnow::error::ParseError::into_inner)
            .whatever_context("parse tile pattern operations")?;
        let b_box = tile.b_box().whatever_context("get tile b_box")?;
        let x_step = tile.x_step().whatever_context("get tile x_step")?;
        let y_step = tile.y_step().whatever_context("get tile y_step")?;

        ensure_whatever!(x_step >= 0.0, "negative x_step not supported");
        ensure_whatever!(y_step >= 0.0, "negative y_step not supported");

        let mut zoom = 1.0f32;
        let (mut w, mut h) = (
            if x_step == 0.0 {
                b_box.width()
            } else {
                b_box.width().min(x_step)
            },
            if y_step == 0.0 {
                b_box.height()
            } else {
                b_box.height().min(y_step)
            },
        );
        if w == 0.0 || h == 0.0 {
            return Ok(());
        }

        let mut matrix = tile.matrix().whatever_context("get tile matrix")?;
        while w > canvas_size.width && h > canvas_size.height {
            w /= 2.0;
            h /= 2.0;
            zoom /= 2.0;
            matrix = matrix.then_scale(2.0, 2.0);
        }

        let resources = tile.resources().whatever_context("get tile resources")?;
        let option = RenderOptionBuilder::default()
            .zoom(zoom)
            .page_box(&b_box, 0)
            .background_color(SkiaColor::TRANSPARENT)
            .build();
        let Some(mut canvas) = option.create_canvas() else {
            return Ok(());
        };
        let Some(mut render) =
            Render::new_nested(self.nested_level, &mut canvas, option, &resources)?
        else {
            return Ok(());
        };
        let color_state = get_state(self)?;
        if let Some(args) = color_args {
            // set color used for paint matrix image
            color_state.set_color_args(args)?;
        }
        ops.into_iter().try_for_each(|op| render.exec(op))?;
        drop(render);
        color_state.paint = PaintCreator::Tile((canvas, matrix, x_step > b_box.width()));
        Ok(())
    }

    fn gen_glyph_path(
        glyph_render: &dyn GlyphRender<SkiaPathSink>,
        gid: u16,
    ) -> Result<PathBuilder> {
        let mut sink = SkiaPathSink(PathBuilder::new());
        glyph_render.render(gid, &mut sink);
        Ok(sink.into_inner())
    }

    fn render_glyph(
        canvas: &mut Pixmap,
        text_clip_path: &mut Path,
        state: &State,
        path: SkiaPath,
        render_mode: TextRenderingMode,
        trans: Transform,
    ) -> Result<()> {
        match render_mode {
            TextRenderingMode::Fill => {
                canvas.fill_path(
                    &path,
                    state.get_fill_paint()?.as_ref(),
                    FillRule::Winding,
                    trans,
                    state.get_mask().as_deref(),
                );
            }
            TextRenderingMode::Stroke => {
                canvas.stroke_path(
                    &path,
                    state.get_stroke_paint()?.as_ref(),
                    state.get_stroke(),
                    trans,
                    state.get_mask().as_deref(),
                );
            }
            TextRenderingMode::FillAndStroke => {
                canvas.fill_path(
                    &path,
                    state.get_fill_paint()?.as_ref(),
                    FillRule::Winding,
                    trans,
                    state.get_mask().as_deref(),
                );
                canvas.stroke_path(
                    &path,
                    state.get_stroke_paint()?.as_ref(),
                    state.get_stroke(),
                    trans,
                    state.get_mask().as_deref(),
                );
            }
            TextRenderingMode::Clip => {
                let path = path.transform(trans).whatever_context("transform path")?;
                text_clip_path.path_builder()?.push_path(&path);
            }
            TextRenderingMode::FillAndClip => {
                canvas.fill_path(
                    &path,
                    state.get_fill_paint()?.as_ref(),
                    FillRule::Winding,
                    trans,
                    state.get_mask().as_deref(),
                );
                let path = path.transform(trans).whatever_context("transform path")?;
                text_clip_path.path_builder()?.push_path(&path);
            }
            TextRenderingMode::StrokeAndClip => {
                canvas.stroke_path(
                    &path,
                    state.get_stroke_paint()?.as_ref(),
                    state.get_stroke(),
                    trans,
                    state.get_mask().as_deref(),
                );
                let path = path.transform(trans).whatever_context("transform path")?;
                text_clip_path.path_builder()?.push_path(&path);
            }
            TextRenderingMode::FillStrokeAndClip => {
                canvas.fill_path(
                    &path,
                    state.get_fill_paint()?.as_ref(),
                    FillRule::Winding,
                    trans,
                    state.get_mask().as_deref(),
                );
                canvas.stroke_path(
                    &path,
                    state.get_stroke_paint()?.as_ref(),
                    state.get_stroke(),
                    trans,
                    state.get_mask().as_deref(),
                );
                let path = path.transform(trans).whatever_context("transform path")?;
                text_clip_path.path_builder()?.push_path(&path);
            }
            _ => {
                whatever!("TODO: Unsupported text rendering mode: {:?}", render_mode);
            }
        }
        Ok(())
    }

    fn show_text(&mut self, text: &[u8]) -> Result<()> {
        let text_object = self.text_object()?;
        if text_object.render_mode == TextRenderingMode::Invisible {
            return Ok(());
        }

        let font_name = text_object
            .font_name
            .as_ref()
            .or_else(|| {
                warn!("font name not set in text_object, use first font in font cache");
                self.font_cache.first_font()
            })
            .whatever_context("get current font name")?;
        let font = self.font_cache.get_font(font_name);
        let op = self.font_cache.get_op(font_name);
        let glyph_width = self.font_cache.get_glyph_width(font_name);
        let state = Self::top(&self.stack)?;
        let mut text_object = state.text_object.clone();
        text_object
            .set_units_per_em(op.units_per_em().whatever_context("get units per em")? as f32);
        text_object.set_write_mode(op.write_mode());
        let user_to_device = state.user_to_device.into_skia();

        if let Some(type3_font) = font.as_type3() {
            let font_matrix = type3_font
                .matrix()
                .whatever_context("get type3 font matrix")?;
            let resources = type3_font
                .resources()
                .whatever_context("get type3 font resources")?;
            let Some(mut render) = Render::new_nested(
                self.nested_level,
                self.canvas,
                RenderOptionBuilder::default()
                    .dimension(self.dimension)
                    .background_color(SkiaColor::TRANSPARENT)
                    .state(state.clone())
                    .build(),
                resources.as_ref().unwrap_or(self.resources),
            )?
            else {
                return Ok(());
            };

            for ch in op.decode_chars(text).whatever_context("decode chars")? {
                Self::current_mut(&mut render.stack)?.set_ctm(
                    text_object
                        .type3_runtime_matrix(&font_matrix)
                        .then(&state.ctm)
                        .with_destination()
                        .with_source(),
                );
                let gid = op.char_to_gid(ch).whatever_context("get char to gid")?;
                if let Some(glyph) = type3_font.get_glyph(gid) {
                    for op in glyph.operations() {
                        render.exec(op.clone())?;
                    }
                }

                text_object.move_to_next_pos(
                    glyph_width
                        .advance(gid as u32, ch)
                        .whatever_context("get char width")?,
                    ch == 32,
                );
            }
        } else {
            let glyph_render = self.font_cache.get_glyph_render(font_name);
            let mut text_clip_path = Path::default();

            for ch in op.decode_chars(text).whatever_context("decode chars")? {
                let gid = op.char_to_gid(ch).whatever_context("get char to gid")?;
                let path = Self::gen_glyph_path(glyph_render, gid)?;
                if !path.is_empty() {
                    let path = path.finish().whatever_context("finish path")?;
                    let path = path
                        .transform(text_object.runtime_matrix().into_skia())
                        .whatever_context("transform by text_object runtime matrix")?;

                    Self::render_glyph(
                        self.canvas,
                        &mut text_clip_path,
                        state,
                        path,
                        text_object.render_mode,
                        user_to_device,
                    )?;
                }

                text_object.move_to_next_pos(
                    glyph_width
                        .advance(gid as u32, ch)
                        .whatever_context("get glyph advance")?,
                    ch == 32,
                );
            }

            if let Some(text_clip_path) = text_clip_path.finish()? {
                text_object
                    .text_clipping_path
                    .path_builder()?
                    .push_path(text_clip_path);
            }
        }
        Self::current_mut(&mut self.stack)?.text_object = text_object;
        Ok(())
    }

    fn show_texts(&mut self, texts: &[TextStringOrNumber]) -> Result<()> {
        for t in texts {
            match t {
                TextStringOrNumber::TextString(s) => self.show_text(s.to_bytes())?,
                TextStringOrNumber::Number(n) => {
                    self.text_object_mut()?.adjust_tj(*n);
                }
            }
        }
        Ok(())
    }

    fn end_text(&mut self) -> Result<()> {
        Self::current_mut(&mut self.stack)?.end_text_object()
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
    write_mode: WriteMode,
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
            write_mode: WriteMode::Horizontal,
        }
    }

    pub fn set_write_mode(&mut self, mode: WriteMode) {
        self.write_mode = mode;
    }

    pub fn type3_runtime_matrix(&self, font_matrix: &GlyphToTextSpace) -> GlyphToUserSpace {
        font_matrix
            .then_scale(self.font_size * self.horiz_scaling.abs(), self.font_size)
            .then(&self.matrix)
    }

    pub fn runtime_matrix(&self) -> GlyphToUserSpace {
        let base_matrix = Transform2D::scale(self.em_ratio.0, self.em_ratio.0)
            .then_scale(self.font_size * self.horiz_scaling, self.font_size);

        match self.write_mode {
            WriteMode::Horizontal => base_matrix.then(&self.matrix),
            WriteMode::Vertical => {
                // 将原点移到字符上方中点
                // 假设字符宽度是em宽度
                let shift_x = -0.5; // 向左移动半个em宽度使原点位于中点
                let shift_y = -1.0; // 向上移动一个em高度到字符顶部

                let vertical_adjust = Transform2D::translation(shift_x, shift_y);

                base_matrix.then(&vertical_adjust).then(&self.matrix)
            }
        }
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

    fn move_to_next_pos(&mut self, glyph_advance: GlyphLength, word_boundary: bool) {
        let mut advance = glyph_advance * self.em_ratio * self.font_size;

        match self.write_mode {
            WriteMode::Horizontal => {
                advance += self.char_spacing;
                if word_boundary {
                    advance += self.word_spacing;
                }
                self.matrix = move_text_space_right(&self.matrix, advance);
            }
            WriteMode::Vertical => {
                advance -= self.char_spacing;
                if word_boundary {
                    advance -= self.word_spacing;
                }
                self.matrix = move_text_space_down(&self.matrix, advance);
            }
        }
    }

    fn adjust_tj(&mut self, tj: Length<f32, ThousandthsOfText>) {
        let n = tj * self.font_size * Scale::new(1.0 / 1000.0);
        match self.write_mode {
            WriteMode::Horizontal => {
                self.matrix = move_text_space_right(&self.matrix, -n);
            }
            WriteMode::Vertical => {
                self.matrix = move_text_space_down(&self.matrix, n);
            }
        }
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

    fn set_text_rise(&mut self, rise: f32) -> Result<()> {
        // Store the previous rise value to calculate the adjustment needed
        let previous_rise = self.rise;
        self.rise = rise;

        // Apply text rise by modifying the text matrix
        // Text rise moves the baseline up (positive) or down (negative)
        // If the previous rise wasn't 0, we need to remove its effect first
        if previous_rise != 0.0 {
            // Create a translation matrix that undoes the previous rise
            let undo_rise =
                Transform2D::<f32, TextSpace, TextSpace>::translation(0.0, -previous_rise);

            // Remove the previous rise effect
            self.matrix = undo_rise.then(&self.matrix);
            self.line_matrix = undo_rise.then(&self.line_matrix);
        }

        // Apply the new rise if it's not zero
        if rise != 0.0 {
            // Create a translation matrix that moves text vertically by the rise amount
            let rise_transform = Transform2D::<f32, TextSpace, TextSpace>::translation(0.0, rise);

            // Apply the rise transformation to both the current matrix and line matrix
            self.matrix = rise_transform.then(&self.matrix);
            self.line_matrix = rise_transform.then(&self.line_matrix);
        }

        Ok(())
    }
}
