//! Rounded tile corners (`corner_radius`): a texture shader that cuts what a window or a
//! tab bar draws to its tile's rounded box, and a pixel shader for the rounded blocks and
//! borders around it. Both run on the GLES renderer every backend uses.

use smithay::{
    backend::renderer::{
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        gles::{
            GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName, UniformType,
            element::PixelShaderElement,
        },
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform, user_data::UserDataMap},
};

use crate::layout::Rect;

/// The default texture shader of smithay, its colour then masked to the rounded box.
const CLIP_SHADER: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

// From texture coordinates to pixels of the box, a row each.
uniform vec3 box_x;
uniform vec3 box_y;
uniform vec2 box_size;
// Top left, top right, bottom right, bottom left.
uniform vec4 radius;

void main() {
    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

    vec3 uv = vec3(v_coords, 1.0);
    vec2 p = vec2(dot(box_x, uv), dot(box_y, uv));
    vec2 c = p - box_size * 0.5;
    float r = c.x < 0.0 ? (c.y < 0.0 ? radius.x : radius.w) : (c.y < 0.0 ? radius.y : radius.z);
    vec2 q = abs(c) - (box_size * 0.5 - r);
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
    color = color * clamp(0.5 - d, 0.0, 1.0);

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// A block of `color` (premultiplied) over its area, cut to the rounded box the area is part
/// of; with a `thickness`, only a ring that wide inside the box's edge.
const RECT_SHADER: &str = r#"
precision highp float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec4 color;
uniform vec2 box_loc;
uniform vec2 box_size;
// Top left, top right, bottom right, bottom left.
uniform vec4 radius;
uniform float thickness;

float coverage(vec2 p, vec2 size, vec4 radii) {
    vec2 c = p - size * 0.5;
    float r = c.x < 0.0 ? (c.y < 0.0 ? radii.x : radii.w) : (c.y < 0.0 ? radii.y : radii.z);
    vec2 q = abs(c) - (size * 0.5 - r);
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
    return clamp(0.5 - d, 0.0, 1.0);
}

void main() {
    vec2 p = box_loc + v_coords * size;
    float c = coverage(p, box_size, radius);
    if (thickness > 0.0)
        c = c - coverage(p - thickness, box_size - 2.0 * thickness, max(radius - thickness, vec4(0.0)));
    vec4 result = color * c * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        result = vec4(0.0, 0.2, 0.0, 0.2) + result * 0.8;
#endif

    gl_FragColor = result;
}
"#;

/// Both programs, compiled once per renderer; `None` when one failed (logged), and the
/// corners stay square.
#[derive(Clone, Default)]
pub struct Shaders {
    pub clip: Option<GlesTexProgram>,
    pub rect: Option<GlesPixelProgram>,
}

impl Shaders {
    pub fn get(renderer: &mut GlesRenderer) -> Shaders {
        if let Some(shaders) = renderer.egl_context().user_data().get::<Shaders>() {
            return shaders.clone();
        }
        let f2 = |name| UniformName::new(name, UniformType::_2f);
        let clip = renderer
            .compile_custom_texture_shader(
                CLIP_SHADER,
                &[
                    UniformName::new("box_x", UniformType::_3f),
                    UniformName::new("box_y", UniformType::_3f),
                    f2("box_size"),
                    UniformName::new("radius", UniformType::_4f),
                ],
            )
            .inspect_err(|e| tracing::warn!("rounded corners off for windows: {e}"))
            .ok();
        let rect = renderer
            .compile_custom_pixel_shader(
                RECT_SHADER,
                &[
                    UniformName::new("color", UniformType::_4f),
                    f2("box_loc"),
                    f2("box_size"),
                    UniformName::new("radius", UniformType::_4f),
                    UniformName::new("thickness", UniformType::_1f),
                ],
            )
            .inspect_err(|e| tracing::warn!("rounded corners off for borders: {e}"))
            .ok();
        let shaders = Shaders { clip, rect };
        renderer.egl_context().user_data().insert_if_missing(|| shaders.clone());
        shaders
    }
}

/// The radius of each corner of a box: top left, top right, bottom right, bottom left. A
/// corner where the box meets a neighbour of its column is 0, square.
pub type Radii = [i32; 4];

/// `radii`, none more than half of the box's sides.
fn fit(radii: Radii, size: (i32, i32)) -> [f32; 4] {
    radii.map(|r| r.min(size.0 / 2).min(size.1 / 2).max(0) as f32)
}

/// What `RECT_SHADER` is given: colour, the area's place in the box, the box's size,
/// radii and ring thickness.
type RectUniforms = ([f32; 4], (i32, i32), (i32, i32), [f32; 4], i32);

/// A rounded block (`RECT_SHADER`), kept between frames: it only damages when it changes.
#[derive(Default)]
pub struct RoundedRect {
    element: Option<PixelShaderElement>,
    uniforms: Option<RectUniforms>,
}

impl RoundedRect {
    /// The block over `area` (output-local), part of the rounded box `outer`, as `color`; a
    /// ring `thickness` wide when that is not 0.
    pub fn update(
        &mut self,
        program: &GlesPixelProgram,
        area: Rect,
        outer: Rect,
        radii: Radii,
        thickness: i32,
        color: [f32; 4],
    ) -> PixelShaderElement {
        let geometry = Rectangle::<i32, Logical>::new((area.x, area.y).into(), (area.w, area.h).into());
        let uniforms = (color, (area.x - outer.x, area.y - outer.y), (outer.w, outer.h), fit(radii, (outer.w, outer.h)), thickness);
        let list = |(color, loc, size, radius, thickness): RectUniforms| {
            vec![
                Uniform::new("color", color),
                Uniform::new("box_loc", (loc.0 as f32, loc.1 as f32)),
                Uniform::new("box_size", (size.0 as f32, size.1 as f32)),
                Uniform::new("radius", (radius[0], radius[1], radius[2], radius[3])),
                Uniform::new("thickness", thickness as f32),
            ]
        };
        let element = self
            .element
            .get_or_insert_with(|| PixelShaderElement::new(program.clone(), geometry, None, 1.0, list(uniforms), Kind::Unspecified));
        element.resize(geometry, None);
        if self.uniforms != Some(uniforms) {
            element.update_uniforms(list(uniforms));
            self.uniforms = Some(uniforms);
        }
        element.clone()
    }
}

/// An element whose texture is cut to a rounded box (`CLIP_SHADER`), as its tile's corners.
#[derive(Debug)]
pub struct Rounded<E> {
    inner: E,
    program: GlesTexProgram,
    /// The size of the texture it draws, which its texture coordinates are relative to.
    texture: Size<i32, Buffer>,
    /// The box, output-local.
    outer: Rectangle<i32, Physical>,
    radii: [f32; 4],
}

impl<E: Element> Rounded<E> {
    pub fn new(inner: E, program: &GlesTexProgram, texture: Size<i32, Buffer>, outer: Rect, radii: Radii) -> Self {
        Rounded {
            inner,
            program: program.clone(),
            texture,
            outer: Rectangle::new((outer.x, outer.y).into(), (outer.w, outer.h).into()),
            radii: fit(radii, (outer.w, outer.h)),
        }
    }

    /// The box's corners, each its radius square, in the coordinates of the element's `geometry`.
    fn corners(&self, geometry: Rectangle<i32, Physical>) -> [Rectangle<i32, Physical>; 4] {
        let [tl, tr, br, bl] = self.radii.map(|r| r.ceil() as i32);
        let (o, size) = (self.outer.loc - geometry.loc, self.outer.size);
        [(0, 0, tl), (size.w - tr, 0, tr), (size.w - br, size.h - br, br), (0, size.h - bl, bl)]
            .map(|(x, y, r)| Rectangle::new((o.x + x, o.y + y).into(), (r, r).into()))
    }
}

/// From texture coordinates of a `texture` drawn at `dst` from `src` with `transform` to
/// pixels of a box at `outer`: the inverse of the texture matrix smithay builds for it, a row
/// for each of x and y.
fn to_box(
    src: Rectangle<f64, Buffer>,
    dst: Rectangle<i32, Physical>,
    texture: Size<i32, Buffer>,
    transform: Transform,
    outer: Point<i32, Physical>,
) -> Option<([f32; 3], [f32; 3])> {
    use glam::{Affine2, Vec2};
    let src_size = transform.transform_size(src.size);
    let mut tex = Affine2::from_scale(Vec2::new((src_size.w / dst.size.w as f64) as f32, (src_size.h / dst.size.h as f64) as f32));
    let (w, h) = (src_size.w as f32, src_size.h as f32);
    let translation = match transform {
        Transform::Normal | Transform::Flipped90 => Vec2::ZERO,
        Transform::_90 => Vec2::new(0.0, w),
        Transform::_180 => Vec2::new(w, h),
        Transform::_270 => Vec2::new(h, 0.0),
        Transform::Flipped => Vec2::new(w, 0.0),
        Transform::Flipped180 => Vec2::new(0.0, h),
        Transform::Flipped270 => Vec2::new(h, w),
    };
    tex = Affine2::from_translation(translation) * transform.matrix() * tex;
    tex = Affine2::from_translation(Vec2::new(src.loc.x as f32, src.loc.y as f32)) * tex;
    tex = Affine2::from_scale(Vec2::new(1.0 / texture.w as f32, 1.0 / texture.h as f32)) * tex;
    if tex.matrix2.determinant() == 0.0 {
        return None;
    }
    let offset = dst.loc - outer;
    let m = Affine2::from_translation(Vec2::new(offset.x as f32, offset.y as f32)) * tex.inverse();
    Some(([m.x_axis.x, m.y_axis.x, m.translation.x], [m.x_axis.y, m.y_axis.y, m.translation.y]))
}

impl<E: Element> Element for Rounded<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn damage_since(&self, scale: Scale<f64>, commit: Option<CommitCounter>) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    /// What the inner element covers, but the corners: what is behind them shows.
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        let corners = self.corners(self.inner.geometry(scale));
        self.inner.opaque_regions(scale).into_iter().flat_map(|r| r.subtract_rects(corners)).collect()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<E: RenderElement<GlesRenderer>> RenderElement<GlesRenderer> for Rounded<E> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        let Some((box_x, box_y)) = to_box(src, dst, self.texture, self.inner.transform(), self.outer.loc) else { return Ok(()) };
        frame.override_default_tex_program(
            self.program.clone(),
            vec![
                Uniform::new("box_x", box_x),
                Uniform::new("box_y", box_y),
                Uniform::new("box_size", (self.outer.size.w as f32, self.outer.size.h as f32)),
                Uniform::new("radius", (self.radii[0], self.radii[1], self.radii[2], self.radii[3])),
            ],
        );
        let result = self.inner.draw(frame, src, dst, damage, opaque_regions, cache);
        frame.clear_tex_program_override();
        result
    }

    /// Never scanned out directly: the plane would show the square corners.
    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where texture coordinates `uv` land in the box.
    fn at(rows: ([f32; 3], [f32; 3]), uv: (f32, f32)) -> (f32, f32) {
        let ([a, b, c], [d, e, f]) = rows;
        (a * uv.0 + b * uv.1 + c, d * uv.0 + e * uv.1 + f)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Buffer> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn a_whole_texture_fills_its_box() {
        let dst = Rectangle::new((10, 20).into(), (200, 100).into());
        let rows = to_box(rect(0.0, 0.0, 200.0, 100.0), dst, (200, 100).into(), Transform::Normal, (10, 20).into()).unwrap();
        assert_eq!(at(rows, (0.0, 0.0)), (0.0, 0.0));
        assert_eq!(at(rows, (1.0, 1.0)), (200.0, 100.0));
    }

    #[test]
    fn a_cropped_texture_keeps_its_place_in_the_box() {
        // The right half of a 200x100 buffer, drawn 50px into a box below a 30px strip.
        let dst = Rectangle::new((60, 50).into(), (100, 100).into());
        let rows = to_box(rect(100.0, 0.0, 100.0, 100.0), dst, (200, 100).into(), Transform::Normal, (10, 20).into()).unwrap();
        assert_eq!(at(rows, (0.5, 0.0)), (50.0, 30.0));
        assert_eq!(at(rows, (1.0, 1.0)), (150.0, 130.0));
    }

    #[test]
    fn a_scaled_buffer_maps_to_pixels() {
        let dst = Rectangle::new((0, 0).into(), (200, 100).into());
        let rows = to_box(rect(0.0, 0.0, 400.0, 200.0), dst, (400, 200).into(), Transform::Normal, (0, 0).into()).unwrap();
        assert_eq!(at(rows, (0.5, 0.5)), (100.0, 50.0));
    }

    #[test]
    fn a_turned_buffer_maps_its_corners_to_the_box() {
        // A 100x200 buffer turned to fill 200x100: every corner of it lands on one of the box.
        let dst = Rectangle::new((0, 0).into(), (200, 100).into());
        let rows = to_box(rect(0.0, 0.0, 100.0, 200.0), dst, (100, 200).into(), Transform::_90, (0, 0).into()).unwrap();
        let mut corners: Vec<(i32, i32)> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .map(|uv| at(rows, uv))
            .map(|(x, y)| (x.round() as i32, y.round() as i32))
            .to_vec();
        corners.sort();
        assert_eq!(corners, vec![(0, 0), (0, 100), (200, 0), (200, 100)]);
    }
}
