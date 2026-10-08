use gpui::{Hsla, Rgba};

mod oklab {
    use gpui::Rgba;

    #[inline]
    fn to_linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    #[inline]
    fn from_linear(c: f32) -> f32 {
        if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    }

    pub fn rgb_to_oklab(rgb: Rgba) -> (f32, f32, f32) {
        let lr = to_linear(rgb.r);
        let lg = to_linear(rgb.g);
        let lb = to_linear(rgb.b);

        let l = 0.412_221_46 * lr + 0.536_332_55 * lg + 0.051_445_995 * lb;
        let m = 0.211_903_5 * lr + 0.680_699_5 * lg + 0.107_396_96 * lb;
        let s = 0.088_302_46 * lr + 0.281_718_85 * lg + 0.629_978_7 * lb;

        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        let l = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
        let a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
        let b = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;

        (l, a, b)
    }

    pub fn oklab_to_rgb(l: f32, a: f32, b: f32) -> Rgba {
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let lr = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
        let lg = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
        let lb = -0.0041960863 * l - 0.703_418_6 * m + 1.707_614_7 * s;

        Rgba {
            r: from_linear(lr).clamp(0.0, 1.0),
            g: from_linear(lg).clamp(0.0, 1.0),
            b: from_linear(lb).clamp(0.0, 1.0),
            a: 1.0,
        }
    }
}

pub trait ColorExt: Sized {
    fn mix(&self, other: Self, factor: f32) -> Self;
}

impl ColorExt for Hsla {
    fn mix(&self, other: Self, factor: f32) -> Self {
        let factor = factor.clamp(0.0, 1.0);
        let inv = 1.0 - factor;

        let result_alpha = self.a * factor + other.a * inv;

        if result_alpha == 0.0 {
            return Self {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0.0,
            };
        }

        let (l1, a1, b1) = oklab::rgb_to_oklab(self.to_rgb());
        let (l2, a2, b2) = oklab::rgb_to_oklab(other.to_rgb());

        let (alpha1, alpha2) = (self.a, other.a);

        let l = (l1 * alpha1 * factor + l2 * alpha2 * inv) / result_alpha;
        let a = (a1 * alpha1 * factor + a2 * alpha2 * inv) / result_alpha;
        let b = (b1 * alpha1 * factor + b2 * alpha2 * inv) / result_alpha;

        let mut rgb: Rgba = oklab::oklab_to_rgb(l, a, b);
        rgb.a = result_alpha;
        rgb.into()
    }
}
