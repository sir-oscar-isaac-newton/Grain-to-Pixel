//! Copying part of one recipe onto another ("apply these settings to the whole roll").

use serde::{Deserialize, Serialize};

use super::{Color, Recipe};

/// A group of settings that can be copied between frames: one panel of the Develop view.
///
/// `Color` carries the colour balance and the black-and-white conversion (`mono`), which live in
/// the same panel, but not the split toning or the colour mixer, which are parts of their own
/// (`SplitTone`, `Mixer`); `Noise` is the noise reduction block; `Base` carries the base
/// source together with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RecipeField {
    Geometry,
    Base,
    Invert,
    Color,
    Mixer,
    SplitTone,
    Tone,
    Dust,
    Noise,
    Output,
}

impl Recipe {
    /// Copies the listed groups of settings from `source` into this recipe; everything else, and
    /// the version, stays as it was.
    pub fn copy_fields_from(&mut self, source: &Recipe, fields: &[RecipeField]) {
        for field in fields {
            match field {
                RecipeField::Geometry => self.geometry = source.geometry,
                RecipeField::Base => self.base = source.base,
                RecipeField::Invert => self.invert = source.invert,
                RecipeField::Color => {
                    // Every colour field but the split toning, which is a part of its own.
                    self.color = Color {
                        split_tone: self.color.split_tone,
                        mixer: self.color.mixer,
                        ..source.color
                    };
                    self.mono = source.mono;
                }
                RecipeField::Mixer => self.color.mixer = source.color.mixer,
                RecipeField::SplitTone => self.color.split_tone = source.color.split_tone,
                RecipeField::Tone => self.tone = source.tone.clone(),
                RecipeField::Dust => self.dust = source.dust,
                RecipeField::Noise => self.noise = source.noise,
                RecipeField::Output => self.output = source.output,
            }
        }
    }
}

#[cfg(test)]
// Tests start from the default recipe and set only the field under test, which reads best.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    /// A recipe with every group set to something other than its default.
    fn edited() -> Recipe {
        let mut r = Recipe::default();
        r.geometry.angle_deg = 2.0;
        r.geometry.rot90 = 1;
        r.base.rgb = [0.8, 0.5, 0.3];
        r.invert.black = [0.1, 0.1, 0.1];
        r.color.temperature = 12.0;
        r.color.split_tone.highlight_strength = 40.0;
        r.color.split_tone.shadow_hue = 200.0;
        r.color.mixer.hue[1] = 25.0;
        r.color.mixer.saturation[5] = -40.0;
        r.color.mixer.lightness[7] = 10.0;
        r.mono.enabled = true;
        r.tone.exposure_ev = 0.7;
        r.tone.curve.master = vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]];
        // Dust removal is on by default, so turn it off to make a difference.
        r.dust.enabled = false;
        r.dust.threshold = 0.3;
        r.noise.luminance = 35.0;
        r.noise.color = 60.0;
        r.output.space = super::super::OutputSpace::Prophoto;
        r
    }

    #[test]
    fn each_field_copies_its_own_settings_and_nothing_else() {
        let source = edited();
        let all = [
            RecipeField::Geometry,
            RecipeField::Base,
            RecipeField::Invert,
            RecipeField::Color,
            RecipeField::Mixer,
            RecipeField::SplitTone,
            RecipeField::Tone,
            RecipeField::Dust,
            RecipeField::Noise,
            RecipeField::Output,
        ];
        for field in all {
            let mut target = Recipe::default();
            target.copy_fields_from(&source, &[field]);
            let mut want = Recipe::default();
            match field {
                RecipeField::Geometry => want.geometry = source.geometry,
                RecipeField::Base => want.base = source.base,
                RecipeField::Invert => want.invert = source.invert,
                RecipeField::Color => {
                    want.color = Color {
                        split_tone: Default::default(),
                        mixer: Default::default(),
                        ..source.color
                    };
                    want.mono = source.mono;
                }
                RecipeField::Mixer => want.color.mixer = source.color.mixer,
                RecipeField::SplitTone => want.color.split_tone = source.color.split_tone,
                RecipeField::Tone => want.tone = source.tone.clone(),
                RecipeField::Dust => want.dust = source.dust,
                RecipeField::Noise => want.noise = source.noise,
                RecipeField::Output => want.output = source.output,
            }
            assert_eq!(target, want, "{field:?}");
            assert_ne!(target, Recipe::default(), "{field:?} changed something");
        }
    }

    #[test]
    fn copying_every_field_makes_the_recipes_equal_and_keeps_the_target_version() {
        let source = edited();
        let mut target = Recipe::default();
        target.v = source.v + 100;
        target.copy_fields_from(
            &source,
            &[
                RecipeField::Geometry,
                RecipeField::Base,
                RecipeField::Invert,
                RecipeField::Color,
                RecipeField::Mixer,
                RecipeField::SplitTone,
                RecipeField::Tone,
                RecipeField::Dust,
                RecipeField::Noise,
                RecipeField::Output,
            ],
        );
        assert_eq!(target.v, source.v + 100);
        target.v = source.v;
        assert_eq!(target, source);
    }

    #[test]
    fn no_fields_changes_nothing_and_other_settings_stay() {
        let mut target = Recipe::default();
        target.tone.exposure_ev = -1.0;
        target.copy_fields_from(&edited(), &[]);
        assert_eq!(target.tone.exposure_ev, -1.0);

        target.copy_fields_from(&edited(), &[RecipeField::Geometry]);
        assert_eq!(target.tone.exposure_ev, -1.0, "tone is not geometry");
        assert_eq!(target.geometry.angle_deg, 2.0);
    }

    #[test]
    fn colour_and_split_toning_are_independent_parts() {
        let source = edited();
        let mut target = Recipe::default();
        target.color.split_tone.balance = -30.0;
        target.copy_fields_from(&source, &[RecipeField::Color]);
        assert_eq!(target.color.temperature, 12.0);
        assert!(target.mono.enabled);
        assert_eq!(
            target.color.split_tone,
            crate::recipe::SplitTone {
                balance: -30.0,
                ..Default::default()
            },
            "the colour part leaves the split toning alone"
        );

        let mut target = Recipe::default();
        target.color.temperature = -5.0;
        target.mono.enabled = false;
        target.copy_fields_from(&source, &[RecipeField::SplitTone]);
        assert_eq!(target.color.split_tone, source.color.split_tone);
        assert_eq!(target.color.temperature, -5.0, "split toning is not colour");
        assert!(!target.mono.enabled);
    }

    #[test]
    fn colour_and_the_mixer_are_independent_parts() {
        let source = edited();
        let mut own = Recipe::default();
        own.color.mixer.hue[3] = -15.0;
        let mut target = own.clone();
        target.copy_fields_from(&source, &[RecipeField::Color]);
        assert_eq!(target.color.temperature, 12.0);
        assert_eq!(
            target.color.mixer, own.color.mixer,
            "the colour part leaves the mixer alone"
        );

        let mut target = Recipe::default();
        target.color.temperature = -5.0;
        target.copy_fields_from(&source, &[RecipeField::Mixer]);
        assert_eq!(target.color.mixer, source.color.mixer);
        assert_eq!(target.color.temperature, -5.0, "the mixer is not colour");
        assert_eq!(target.color.split_tone, Default::default());
    }

    #[test]
    fn the_noise_part_copies_the_noise_block_and_nothing_else() {
        let source = edited();
        let mut own = Recipe::default();
        own.tone.exposure_ev = 0.3;
        own.output.sharpen.amount = 50.0;
        let mut target = own.clone();
        target.copy_fields_from(&source, &[RecipeField::Noise]);
        assert_eq!(target.noise, source.noise);
        assert_eq!(target.tone, own.tone);
        assert_eq!(target.output, own.output);

        // Other parts leave the target's own noise reduction alone, in both directions.
        let mut own = Recipe::default();
        own.noise.color = 20.0;
        let mut target = own.clone();
        target.copy_fields_from(&source, &[RecipeField::Output, RecipeField::Color]);
        assert_eq!(target.noise, own.noise);
    }
}
