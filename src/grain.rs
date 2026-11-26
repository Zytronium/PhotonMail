//! Subtle grainy-gradient surface texture.
//!
//! Instead of drawing a noise layer *on top of* the UI (which needs absolute
//! positioning and layering to work), the gradient and the grain are baked
//! into a single SkSL shader that is used as the `.background()` of the
//! panes themselves. Rows inside a pane have no background of their own while
//! idle, so the texture shows through them.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::atomic::{AtomicU16, Ordering},
};

use freya::engine::prelude::{Data, RuntimeEffect};
use freya::prelude::*;

use crate::theme::{DARK, Rgb};

/// How strong the grain is, as a fraction of full brightness.
/// Raise for more texture, lower for less.
const GRAIN: f32 = 0.011;
static STRENGTH: AtomicU16 = AtomicU16::new(100);

const SKSL: &str = r#"
uniform float ox;
uniform float oy;
uniform float w;
uniform float h;
uniform float r0;
uniform float g0;
uniform float b0;
uniform float r1;
uniform float g1;
uniform float b1;
uniform float grain;

// Sine-free hash: stable on every GPU, no precision banding.
float hash12(float2 p) {
  float3 p3 = fract(float3(p.x, p.y, p.x) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

half4 main(float2 xy) {
  // Position inside the pane, 0..1. Clamped so that if the coordinates are
  // ever not window-space the gradient degrades to a flat colour instead of
  // breaking; the grain below does not depend on this.
  float2 uv = clamp((xy - float2(ox, oy)) / float2(max(w, 1.0), max(h, 1.0)), 0.0, 1.0);

  // Diagonal gradient, top-left -> bottom-right.
  float t = clamp(dot(uv, float2(0.35, 0.65)), 0.0, 1.0);
  float3 base = mix(float3(r0, g0, b0), float3(r1, g1, b1), t);

  // Triangular-distributed 1px luminance noise, roughly -1..1.
  float2 cell = floor(xy);
  float n = hash12(cell) + hash12(cell + 17.0) - 1.0;

  base += n * grain;
  return half4(half3(clamp(base, 0.0, 1.0)), 1.0);
}
"#;

fn build(top: Rgb, bottom: Rgb, grain: f32) -> Fill {
    let effect =
        RuntimeEffect::make_for_shader(SKSL, None).expect("grain shader failed to compile");

    ShaderFill::new(SKSL, effect, move |effect, bounds| {
        let c = |v: u8| v as f32 / 255.0;
        let values = [
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
            c(top.0),
            c(top.1),
            c(top.2),
            c(bottom.0),
            c(bottom.1),
            c(bottom.2),
            grain,
        ];

        let mut data: Vec<u8> = Vec::with_capacity(values.len() * 4);
        for v in values {
            data.extend_from_slice(&v.to_ne_bytes());
        }
        // Skia wants exactly `uniform_size()` bytes.
        data.resize(effect.uniform_size(), 0);

        let shader = effect.make_shader(Data::new_copy(&data), &[], None);
        if shader.is_none() {
            eprintln!(
                "grain: make_shader failed (uniform_size = {}, supplied = {})",
                effect.uniform_size(),
                values.len() * 4
            );
        }
        shader
    })
    .into()
}

/// Lighten `base` toward the accent at the top-left and darken it toward the
/// bottom-right.
fn gradient(base: Rgb) -> (Rgb, Rgb) {
    let add = |v: u8, d: i16| (v as i16 + d).clamp(0, 255) as u8;
    (
        (add(base.0, 7), add(base.1, 5), add(base.2, 13)),
        (add(base.0, -4), add(base.1, -4), add(base.2, -5)),
    )
}

// Cached per thread: `ShaderFill` compares by pointer, so building a new one on
// every render would make Freya think the background changed every frame.
thread_local! {
    static PANEL: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static SURFACE: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static TITLEBAR: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static HOVER: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static SELECTED: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static CLOSE_HOVER: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
    static DIALOG: RefCell<HashMap<u16, Fill>> = RefCell::new(HashMap::new());
}

pub fn set_strength(multiplier: f32) {
    let hundredths = (multiplier.clamp(0.0, 2.0) * 100.0).round() as u16;
    STRENGTH.store(hundredths, Ordering::Relaxed);
}

fn fill(cache: &RefCell<HashMap<u16, Fill>>, base: Rgb) -> Fill {
    let strength = STRENGTH.load(Ordering::Relaxed);
    cache
        .borrow_mut()
        .entry(strength)
        .or_insert_with(|| {
            let (top, bottom) = gradient(base);
            build(top, bottom, grain_amount(f32::from(strength) / 100.0))
        })
        .clone()
}

fn grain_amount(multiplier: f32) -> f32 {
    GRAIN * multiplier
}

/// Sidebar and reader.
pub fn panel() -> Fill {
    PANEL.with(|cache| fill(cache, DARK.panel))
}

/// Message list.
pub fn surface() -> Fill {
    SURFACE.with(|cache| fill(cache, DARK.bg))
}

/// Window titlebar.
pub fn titlebar() -> Fill {
    TITLEBAR.with(|cache| fill(cache, DARK.titlebar))
}

/// Hovered / secondary-button background.
pub fn hover() -> Fill {
    HOVER.with(|cache| fill(cache, DARK.hover))
}

/// Selected row background.
pub fn selected() -> Fill {
    SELECTED.with(|cache| fill(cache, DARK.selected))
}

/// Window close button while hovered.
pub fn close_hover() -> Fill {
    CLOSE_HOVER.with(|cache| fill(cache, DARK.close_hover))
}

/// Popup dialog card.
pub fn dialog() -> Fill {
    DIALOG.with(|cache| fill(cache, DARK.panel))
}

#[cfg(test)]
mod tests {
    use super::{GRAIN, grain_amount};

    #[test]
    fn grain_multiplier_scales_shader_noise_amplitude_continuously() {
        assert_eq!(grain_amount(0.0), 0.);
        assert_eq!(grain_amount(0.5), GRAIN * 0.5);
        assert_eq!(grain_amount(1.0), GRAIN);
        assert_eq!(grain_amount(1.23), GRAIN * 1.23);
        assert_eq!(grain_amount(2.0), GRAIN * 2.);
    }
}
