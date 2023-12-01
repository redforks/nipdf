use crate::{
    file::{
        page::{GraphicsStateParameterDict, Operation, PageContent, Rectangle, ResourceDict},
        XObjectDict, XObjectType,
    },
    function::Domain,
    graphics::{
        color_space::{ColorSpace, ColorSpaceTrait},
        parse_operations,
        shading::{build_shading, Axial, Extend, Radial, Shading},
        trans::{
            image_to_user_space, logic_device_to_device, move_text_space_pos,
            move_text_space_right, to_device_space, ImageToDeviceSpace, IntoSkiaTransform,
            LogicDeviceToDeviceSpace, TextToUserSpace, UserToDeviceSpace, UserToLogicDeviceSpace,
            UserToUserSpace,
        },
        ColorArgs, ColorArgsOrName, LineCapStyle, LineJoinStyle, NameOfDict, PatternType, Point,
        RenderingIntent, ShadingPatternDict, TextRenderingMode, TilingPatternDict,
    },
    object::{Object, PdfObject, TextStringOrNumber},
};
use anyhow::{Ok, Result as AnyResult};
use educe::Educe;
use either::Either::{self, Left, Right};
use image::RgbaImage;
use log::{debug, info};
use nom::{combinator::eof, sequence::terminated};
use prescript::Name;
use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    collections::VecDeque,
    convert::AsRef,
    rc::Rc,
};
use tiny_skia::{
    Color as SkiaColor, FillRule, FilterQuality, Mask, MaskType, Paint, Path as SkiaPath,
    PathBuilder, Pixmap, PixmapPaint, PixmapRef, Point as SkiaPoint, Rect, Stroke, StrokeDash,
    Transform,
};

mod fonts;
use euclid::Angle;
use fonts::*;

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
    Color(SkiaColor),
    Gradient(Paint<'static>),
    Tile((Pixmap, UserToLogicDeviceSpace)),
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
                let transform = to_device_space(height, 1.0, matrix);
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
}

impl ColorState {
    pub fn set_color_args(&mut self, color_args: impl AsRef<[f32]>) {
        let color = self.color_space.to_skia_color(color_args.as_ref());
        self.set_color(color);
    }

    pub fn set_color(&mut self, color: SkiaColor) {
        self.set_paint(PaintCreator::Color(color), None);
    }

    pub fn set_paint(&mut self, paint: PaintCreator, background_color: Option<SkiaColor>) {
        self.background_paint = background_color.map(PaintCreator::Color);
        self.paint = paint;
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
            canvas.stroke_path(path, &paint.create(), stroke, transform, mask);
        }
        canvas.stroke_path(path, &self.paint.create(), stroke, transform, mask);
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
            canvas.fill_path(path, &paint.create(), fill_rule, transform, mask);
        }
        canvas.fill_path(path, &self.paint.create(), fill_rule, transform, mask);
    }
}

#[derive(Debug, Clone)]
struct State {
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

    fn set_ctm(&mut self, ctm: UserToLogicDeviceSpace) {
        self.ctm = self.dimension.transform.then(&ctm);
        self.user_to_device = self.ctm.then(&self.dimension.logic_device_to_device());
    }

    fn concat_ctm(&mut self, ctm: UserToUserSpace) {
        self.ctm = ctm.then(&self.ctm);
        self.user_to_device = self.ctm.then(&self.dimension.logic_device_to_device());
        debug!("ctm to {:?}", self.ctm);
        debug!("user_to_device to {:?}", self.user_to_device);
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

    fn get_fill_paint(&self) -> Cow<'_, Paint<'_>> {
        self.fill_state.paint.create()
    }

    fn get_stroke_paint(&self) -> Cow<'_, Paint<'_>> {
        self.stroke_state.paint.create()
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> ImageToDeviceSpace {
        image_to_user_space(img_w, img_h)
            .then(&self.ctm)
            .then(&self.dimension.logic_device_to_device())
    }

    fn get_mask(&self) -> Option<Ref<Mask>> {
        self.mask.as_ref().map(|m| m.1.borrow())
    }

    fn set_graphics_state(&mut self, res: &GraphicsStateParameterDict) {
        for key in res.d.dict().keys() {
            match key.as_str() {
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

    fn update_mask(&mut self, path: &SkiaPath, rule: FillRule, flip_y: bool) {
        let w = self.dimension.canvas_width();
        let h = self.dimension.canvas_height();
        let new_mask = || {
            let mut r = Mask::new(w, h).unwrap();
            let p = PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap());
            r.fill_path(&p, FillRule::Winding, true, Transform::identity());
            r
        };

        let mut path = path.clone();
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
        todo!("text knockout");
    }

    pub fn end_text_object(&mut self) {
        // if exists text clipping path, intersection to current clipping path using Winding fill
        // rule
        let p = self.text_object.text_clipping_path.finish();
        if let Some(p) = p {
            let p = p.to_owned();
            self.clip_non_zero(&p, false);
            self.text_object.text_clipping_path.reset();
        }
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

#[derive(Debug, Educe, Clone, Copy)]
#[educe(Default)]
pub struct PageDimension {
    #[educe(Default = 1.0)]
    zoom: f32,
    width: u32,
    height: u32,
    // apply before ctm to handle crop_box/media_box left-bottom not at (0, 0) and page rotate
    transform: UserToUserSpace,
    rotate: i32,
}

impl PageDimension {
    pub fn update(&mut self, dimension: &Rectangle, rotate: i32) {
        self.rotate = rotate % 360;

        let mut transform = UserToUserSpace::identity();
        if dimension.left_x != 0.0 || dimension.lower_y != 0.0 {
            transform = transform.then_translate((-dimension.left_x, -dimension.lower_y).into());
        }
        self.transform = transform;

        self.width = dimension.width() as u32;
        self.height = dimension.height() as u32;
        if self.swap_wh() {
            std::mem::swap(&mut self.width, &mut self.height);
        }
    }

    pub fn canvas_width(&self) -> u32 {
        (self.width as f32 * self.zoom) as u32
    }

    pub fn canvas_height(&self) -> u32 {
        (self.height as f32 * self.zoom) as u32
    }

    fn swap_wh(&self) -> bool {
        self.rotate.abs() == 90 || self.rotate.abs() == 270
    }

    pub fn logic_device_to_device(&self) -> LogicDeviceToDeviceSpace {
        if self.rotate != 0 {
            let (w, h) = if self.swap_wh() {
                (self.height, self.width)
            } else {
                (self.width, self.height)
            };

            let r = logic_device_to_device(h, self.zoom);
            r.then_translate((w as f32 * self.zoom * -0.5, h as f32 * self.zoom * -0.5).into())
                .then_rotate(Angle::degrees(self.rotate as f32))
                .then_translate((h as f32 * self.zoom * 0.5, w as f32 * self.zoom * 0.5).into())
        } else {
            logic_device_to_device(self.height, self.zoom)
        }
    }
}

/// Option for Render
#[derive(Debug, Educe, Clone)]
#[educe(Default)]
pub struct RenderOption {
    /// If crop is specified, the output canvas will be cropped to the specified rectangle.
    crop: Option<Rectangle>,
    #[educe(Default(expression = "SkiaColor::WHITE"))]
    background_color: SkiaColor,
    /// Initial state, used in paint_x_form to pass parent state to form Render.
    state: Option<State>,
    rotate: i32,
    dimension: PageDimension,
}

impl RenderOption {
    pub fn create_canvas(&self) -> Pixmap {
        let mut r = Pixmap::new(
            self.dimension.canvas_width(),
            self.dimension.canvas_height(),
        )
        .unwrap();
        if self.background_color.is_opaque() {
            r.fill(self.background_color);
        }
        r
    }

    /// Convert canvas to image, crop if crop option not None
    pub fn to_image(&self, canvas: Pixmap) -> RgbaImage {
        RgbaImage::from_raw(canvas.width(), canvas.height(), canvas.take()).unwrap()
    }
}

#[derive(Educe)]
#[educe(Default(new))]
pub struct RenderOptionBuilder(RenderOption);

impl RenderOptionBuilder {
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.0.dimension.zoom = zoom;
        self
    }

    pub fn page_box(mut self, dimension: &Rectangle, rotate_degree: i32) -> Self {
        self.0.dimension.update(dimension, rotate_degree);
        self
    }

    fn dimension(mut self, dimension: PageDimension) -> Self {
        self.0.dimension = dimension;
        self
    }

    pub fn crop(mut self, rect: Option<Rectangle>) -> Self {
        self.0.crop = rect;
        self
    }

    pub fn background_color(mut self, color: SkiaColor) -> Self {
        self.0.background_color = color;
        self
    }

    pub fn rotate(mut self, rotate: i32) -> Self {
        self.0.rotate = rotate;
        self
    }

    fn state(mut self, state: State) -> Self {
        self.0.state = Some(state);
        self
    }

    pub fn build(self) -> RenderOption {
        self.0
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Render<'a, 'b, 'c> {
    canvas: &'c mut Pixmap,
    stack: Vec<State>,
    path: Path,
    #[educe(Debug(ignore))]
    font_cache: FontCache<'c>,
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
            state.clip_non_zero(&PathBuilder::from_rect(rect.into()), true);
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
        self.stack.pop().unwrap();
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
            Operation::MoveToStartOfNextLine => {
                let leading = self.stack.last().unwrap().text_object.leading;
                self.text_object_mut()
                    .move_text_position(Point::new(0.0, -leading));
            }

            // Text Showing Operations
            Operation::ShowText(text) => self.show_text(text.to_bytes().unwrap()),
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

            _ => todo!("{:?}", op),
        }
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

        fn load_image_as_mask<'a, 'b>(
            page_width: u32,
            page_height: u32,
            img_dict: &XObjectDict<'a, 'b>,
            resources: &ResourceDict<'a, 'b>,
            state: &State,
            s_mask: bool,
        ) -> AnyResult<Mask> {
            let paint = PixmapPaint {
                quality: FilterQuality::Nearest,
                ..Default::default()
            };

            let mut canvas = Pixmap::new(page_width, page_height).unwrap();
            let img = img_dict.as_stream().unwrap();
            let img = img.decode_image(resources.resolver(), Some(resources))?;
            let mut img = img.into_rgba8();
            img.pixels_mut()
                .for_each(|p| p[3] = if s_mask { p[0] } else { 255 - p[0] });

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

        let state = self.stack.last().unwrap();

        if x_object.image_mask()? {
            let mask = load_image_as_mask(
                self.device_width(),
                self.device_height(),
                x_object,
                self.resources,
                state,
                false,
            )?;
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
            load_image_as_mask(
                self.device_width(),
                self.device_height(),
                &s_mask,
                self.resources,
                state,
                true,
            )
            .unwrap()
        });

        let paint = PixmapPaint {
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
        let mut inner_state = self.stack.last().unwrap().clone();
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
        let (shader_ctm, fill_ctm, rect) = if let Some(b_box) = b_box {
            (Transform::identity(), ctm, b_box)
        } else {
            (
                ctm,
                Transform::identity(),
                Rectangle::from_xywh(
                    0.,
                    0.,
                    self.device_width() as f32,
                    self.device_height() as f32,
                ),
            )
        };

        if let Some(shader) = axial.into_skia(shader_ctm) {
            let paint = Paint {
                shader,
                ..Default::default()
            };
            self.canvas
                .fill_rect(rect.into(), &paint, fill_ctm, state.get_mask().as_deref());
        }
        Ok(())
    }

    fn paint_radial(&mut self, radial: &Radial) -> AnyResult<()> {
        let Domain { start: t0, end: t1 } = radial.domain;
        let Point { x: x0, y: y0 } = radial.start.point;
        let Point { x: x1, y: y1 } = radial.end.point;
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
            let c = radial.color_space.to_rgba(c.as_slice());
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
            let Point { x, y } = radial.start.point;
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
                    .map(|args| get_state(self).color_space.to_skia_color(args.as_ref()));
                let pattern = self.resources.pattern()?;
                let pattern = &pattern[name];
                match pattern.pattern_type()? {
                    PatternType::Tiling => {
                        Self::tiling_pattern(get_state(self), pattern.tiling_pattern()?, color)
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
            Some(cs.to_skia_color(args.as_ref()))
        } else {
            None
        };

        Ok(match build_shading(&shading, resources)? {
            Some(Shading::Axial(axial)) => {
                assert_eq!(Extend::new(true, true), axial.extend);
                axial.into_skia(pattern.matrix()?.into_skia())
            }
            Some(Shading::Radial(radial)) => radial.into_skia(pattern.matrix()?.into_skia()),
            None => return Ok(None),
        }
        .map(|shader| {
            (
                PaintCreator::Gradient(Paint {
                    shader,
                    ..Default::default()
                }),
                background_color,
            )
        }))
    }

    fn tiling_pattern(
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

        let resources = tile.resources()?;
        let option = RenderOptionBuilder::default().page_box(&b_box, 0).build();
        let mut canvas = option.create_canvas();
        let mut render = Render::new(&mut canvas, option, &resources);
        if let Some(color) = color {
            // set color used for paint matrix image
            color_state.set_color(color);
        }
        ops.into_iter().for_each(|op| render.exec(op));
        drop(render);
        color_state.paint = PaintCreator::Tile((canvas, tile.matrix()?));
        Ok(())
    }

    fn gen_glyph_path(glyph_render: &dyn GlyphRender, gid: u16, font_size: f32) -> PathBuilder {
        let mut path = PathBuilder::new();
        let mut sink = PathSink(&mut path);
        glyph_render.render(gid, font_size, &mut sink).unwrap();
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
        let op = self
            .font_cache
            .get_op(self.text_object().font_name.as_ref().unwrap())
            .unwrap();
        let state = self.stack.last().unwrap();

        let text_object = &state.text_object;
        let font_size = text_object.font_size;
        let glyph_render = self
            .font_cache
            .get_glyph_render(self.text_object().font_name.as_ref().unwrap())
            .unwrap();

        let mut text_to_user_space: TextToUserSpace = text_object.matrix;
        let render_mode = text_object.render_mode;
        let mut text_clip_path = Path::default();
        let flip_y = state.user_to_device.into_skia();
        for ch in op.decode_chars(text) {
            let width = font_size.mul_add(
                op.char_width(ch) as f32 / op.units_per_em() as f32,
                char_spacing + if ch == 32 { word_spacing } else { 0.0 },
            );

            let gid = op.char_to_gid(ch);
            let path = Self::gen_glyph_path(glyph_render, gid, font_size);
            if !path.is_empty() {
                let path = path.finish().unwrap();
                // pre transform path to user space, render_glyph() will zoom line_width,
                // pdf line_width state is in user space, but skia line_width is in device space
                // so we need to transform path to user space, and zoom line_width in device space
                let path = path.transform(text_to_user_space.into_skia()).unwrap();

                Self::render_glyph(
                    self.canvas,
                    &mut text_clip_path,
                    state,
                    path,
                    render_mode,
                    flip_y,
                );
            }
            text_to_user_space = move_text_space_right(&text_to_user_space, width);
        }
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
                TextStringOrNumber::TextString(s) => self.show_text(s.to_bytes().unwrap()),
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

#[derive(Educe, Clone)]
#[educe(Debug)]
struct TextObject {
    matrix: TextToUserSpace,
    line_matrix: TextToUserSpace,
    font_size: f32,
    font_name: Option<Name>,
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

    fn set_font(&mut self, nm: NameOfDict, size: f32) {
        self.font_size = size;
        self.font_name = Some(nm.0);
    }

    fn move_text_position(&mut self, p: Point) {
        let matrix = move_text_space_pos(&self.line_matrix, p.x, p.y);
        self.matrix = matrix;
        self.line_matrix = matrix;
    }

    fn set_text_matrix(&mut self, m: TextToUserSpace) {
        self.matrix = m;
        self.line_matrix = m;
    }

    fn move_right(&mut self, n: f32) {
        let tx = -n * 0.001 * self.font_size;
        self.matrix = move_text_space_right(&self.matrix, tx);
    }

    fn set_character_spacing(&mut self, spacing: f32) {
        self.char_spacing = spacing;
    }

    fn set_word_spacing(&mut self, spacing: f32) {
        self.word_spacing = spacing;
    }

    fn set_horizontal_scaling(&mut self, scale: f32) {
        self.horiz_scaling = scale;
        if scale != 100.0 {
            todo!("text horizontal scaling");
        }
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
