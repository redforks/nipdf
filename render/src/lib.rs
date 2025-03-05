use educe::Educe;
use either::Either;
use euclid::Angle;
use image::RgbaImage;
use log::warn;
use nipdf::{
    file::{Page, Rectangle},
    graphics::trans::{LogicDeviceToDeviceSpace, UserToUserSpace, logic_device_to_device},
};
use prescript::Result;
use snafu::{OptionExt, ResultExt};
use tiny_skia::{Color, Pixmap};

mod render;
mod shading;
use render::{Render, State};
mod into_skia;
pub(crate) use into_skia::*;
use num_traits::ToPrimitive;

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

        // width and height are always positive, it is acceptable if it panic because of too
        // large page size
        #[allow(clippy::unwrap_used)]
        {
            self.width = dimension.width().to_u32().unwrap();
            self.height = dimension.height().to_u32().unwrap();
        }
        if self.swap_wh() {
            std::mem::swap(&mut self.width, &mut self.height);
        }
    }

    pub fn canvas_width(&self) -> u32 {
        // width and zoom are always positive, it is acceptable if it panic because of too
        // large page size
        #[allow(clippy::unwrap_used)]
        (self.width as f32 * self.zoom).to_u32().unwrap()
    }

    pub fn canvas_height(&self) -> u32 {
        // height and zoom are always positive, it is acceptable if it panic because of too
        // large page size
        #[allow(clippy::unwrap_used)]
        (self.height as f32 * self.zoom).to_u32().unwrap()
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
    #[educe(Default(expression = Color::WHITE))]
    background_color: Color,
    /// Initial state, used in paint_x_form to pass parent state to form Render.
    state: Option<State>,
    rotate: i32,
    dimension: PageDimension,
    /// If true, operations that result in errors will cause the render to fail immediately.
    /// If false, errors will be logged and rendering will continue.
    #[educe(Default = false)]
    fail_fast: bool,
}

impl RenderOption {
    pub fn create_canvas(&self) -> Option<Pixmap> {
        let (w, h) = (
            self.dimension.canvas_width(),
            self.dimension.canvas_height(),
        );
        if w * h > 1024 * 1024 * 100 {
            log::error!("Cannot create canvas, size too large: {}x{}", w, h);
            return None;
        }

        let mut r = Pixmap::new(w, h)?;
        if self.background_color.is_opaque() {
            r.fill(self.background_color);
        }
        Some(r)
    }

    /// Convert canvas to image, crop if crop option not None
    pub fn to_image(&self, canvas: Pixmap) -> Result<RgbaImage> {
        RgbaImage::from_raw(canvas.width(), canvas.height(), canvas.take())
            .whatever_context("Failed create canvas")
    }
}
#[derive(Educe)]
#[educe(Default(new))]
pub struct RenderOptionBuilder(RenderOption);

impl RenderOptionBuilder {
    /// Set zoom field, if zoom less than 0, it will be ignored.
    pub fn zoom(mut self, zoom: f32) -> Self {
        if zoom <= 0.0 {
            warn!("zoom less than 0, ignored");
            return self;
        }
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

    pub fn background_color(mut self, color: Color) -> Self {
        self.0.background_color = color;
        self
    }

    pub fn rotate(mut self, rotate: i32) -> Self {
        self.0.rotate = rotate;
        self
    }

    pub fn fail_fast(mut self, fail_fast: bool) -> Self {
        self.0.fail_fast = fail_fast;
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

pub fn render_page(page: &Page<'_>, option: RenderOptionBuilder) -> Result<RgbaImage> {
    render_steps(page, option, None, false)
}

pub fn render_steps(
    page: &Page<'_>,
    option: RenderOptionBuilder,
    steps: Option<usize>,
    no_crop: bool,
) -> Result<RgbaImage> {
    let media_box = page.media_box().whatever_context("get page media box")?;
    let crop_box = page.crop_box().whatever_context("get page crop box")?;
    let mut canvas_box = crop_box;
    // if canvas is empty, use default A4 size
    if canvas_box.width() == 0.0 || canvas_box.height() == 0.0 {
        canvas_box = Rectangle::from_xywh(0.0, 0.0, 597.6, 842.4);
    }
    let option = option
        .page_box(&canvas_box, page.rotate())
        .crop((!no_crop && need_crop(crop_box, media_box)).then_some(crop_box))
        .rotate(page.rotate())
        .build();
    let content = page.content().whatever_context("get page content")?;
    let ops = content
        .operations()
        .whatever_context("get page operations")?;
    let Some(mut canvas) = option.create_canvas() else {
        return Ok(RgbaImage::new(0, 0));
    };
    if !ops.is_empty() {
        // skip render if no operations, fixes incorrect pdf files that no resources
        let resource = page.resources().whatever_context("get page resources")?;
        let mut renderer = Render::new(&mut canvas, option.clone(), &resource)?;

        let iter = if let Some(steps) = steps {
            Either::Left(ops.into_iter().take(steps))
        } else {
            Either::Right(ops.into_iter())
        };

        for op in iter {
            match renderer.exec(op) {
                Ok(_) => (),
                Err(e) if option.fail_fast => return Err(e),
                Err(e) => log::error!("Operation failed: {}", e),
            }
        }
    }
    option.to_image(canvas)
}

fn need_crop(crop: Rectangle, media: Rectangle) -> bool {
    crop != media
}

#[cfg(test)]
mod render_tests;
