use educe::Educe;
use euclid::Angle;
use image::RgbaImage;
use nipdf::{
    file::{Page, Rectangle},
    graphics::trans::{logic_device_to_device, LogicDeviceToDeviceSpace, UserToUserSpace},
    object::ObjectValueError,
};
use tiny_skia::{Color, Pixmap};

mod render;
mod shading;
use render::{Render, State};

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
    #[educe(Default(expression = "Color::WHITE"))]
    background_color: Color,
    /// Initial state, used in paint_x_form to pass parent state to form Render.
    state: Option<State>,
    rotate: i32,
    dimension: PageDimension,
}

impl RenderOption {
    pub fn create_canvas(&self) -> Pixmap {
        let (w, h) = (
            self.dimension.canvas_width() as u64,
            self.dimension.canvas_height() as u64,
        );
        if w * h > 1024 * 1024 * 100 {
            panic!("page size too large: {}x{}", w, h);
        }

        let mut r = Pixmap::new(w as u32, h as u32).unwrap();
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

    pub fn background_color(mut self, color: Color) -> Self {
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

pub fn render_page(
    page: &Page,
    option: RenderOptionBuilder,
) -> Result<RgbaImage, ObjectValueError> {
    render_steps(page, option, None, false)
}

pub fn render_steps(
    page: &Page,
    option: RenderOptionBuilder,
    steps: Option<usize>,
    no_crop: bool,
) -> Result<RgbaImage, ObjectValueError> {
    let media_box = page.media_box();
    let crop_box = page.crop_box();
    let mut canvas_box = crop_box.unwrap_or(media_box);
    // if canvas is empty, use default A4 size
    if canvas_box.width() == 0.0 || canvas_box.height() == 0.0 {
        canvas_box = Rectangle::from_xywh(0.0, 0.0, 597.6, 842.4);
    }
    let option = option
        .page_box(&canvas_box, page.rotate())
        .crop((!no_crop && need_crop(crop_box, media_box)).then(|| crop_box.unwrap()))
        .rotate(page.rotate())
        .build();
    let content = page.content()?;
    let ops = content.operations();
    let resource = page.resources();
    let mut canvas = option.create_canvas();
    let mut renderer = Render::new(&mut canvas, option.clone(), &resource);
    if let Some(steps) = steps {
        ops.into_iter().take(steps).for_each(|op| renderer.exec(op));
    } else {
        ops.into_iter().for_each(|op| renderer.exec(op));
    };
    drop(renderer);
    let r = option.to_image(canvas);
    Ok(r)
}

fn need_crop(crop: Option<Rectangle>, media: Rectangle) -> bool {
    match crop {
        None => false,
        Some(crop) => crop != media,
    }
}

#[cfg(test)]
mod render_tests;
