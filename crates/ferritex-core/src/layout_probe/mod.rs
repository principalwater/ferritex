use std::path::Path;

use anyhow::Result;

use crate::model::{DocumentLayout, LayoutProbeOutput};

#[cfg(feature = "layout-probe-tectonic")]
mod tectonic;

#[cfg(any(test, feature = "layout-probe-tectonic"))]
const TWIPS_PER_POINT_F64: f64 = 20.0;

/// Probe effective layout/style values with the configured backend.
///
/// When `layout-probe-tectonic` is disabled, this returns an empty probe
/// output, preserving parser-only behavior.
pub fn probe_layout(input_path: &Path, expanded_source: &str) -> Result<LayoutProbeOutput> {
    #[cfg(feature = "layout-probe-tectonic")]
    {
        return tectonic::probe_layout_with_tectonic(input_path, expanded_source);
    }

    #[cfg(not(feature = "layout-probe-tectonic"))]
    {
        let _ = (input_path, expanded_source);
        Ok(LayoutProbeOutput::default())
    }
}

#[cfg(any(test, feature = "layout-probe-tectonic"))]
pub(crate) fn pt_to_twips_rounded(points: f64) -> Option<i32> {
    if !points.is_finite() {
        return None;
    }

    let twips = (points * TWIPS_PER_POINT_F64).round();
    if !twips.is_finite() {
        return None;
    }

    if twips > i32::MAX as f64 || twips < i32::MIN as f64 {
        return None;
    }

    Some(twips as i32)
}

/// Merge probe output and parser layout with strict precedence.
pub fn merge_probe_and_parser_layout(
    probe: &LayoutProbeOutput,
    parser_layout: DocumentLayout,
) -> DocumentLayout {
    probe.merge_into_layout(parser_layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_precedence_probe_over_parser() {
        let parser_layout = DocumentLayout {
            page_margin_left_twips: Some(100),
            font_size_body_hp: Some(24),
            list_label_sep_twips: Some(120),
            ..DocumentLayout::default()
        };
        let probe = LayoutProbeOutput {
            page_margin_left_twips: Some(200),
            font_size_body_hp: Some(28),
            list_label_sep_twips: Some(140),
            ..LayoutProbeOutput::default()
        };

        let merged = merge_probe_and_parser_layout(&probe, parser_layout);

        assert_eq!(merged.page_margin_left_twips, Some(200));
        assert_eq!(merged.font_size_body_hp, Some(28));
        assert_eq!(merged.list_label_sep_twips, Some(140));
    }

    #[test]
    fn merge_keeps_parser_when_probe_missing() {
        let parser_layout = DocumentLayout {
            page_margin_left_twips: Some(100),
            font_size_body_hp: Some(24),
            list_label_sep_twips: Some(120),
            ..DocumentLayout::default()
        };

        let merged = merge_probe_and_parser_layout(&LayoutProbeOutput::default(), parser_layout);

        assert_eq!(merged.page_margin_left_twips, Some(100));
        assert_eq!(merged.font_size_body_hp, Some(24));
        assert_eq!(merged.list_label_sep_twips, Some(120));
    }

    #[test]
    fn merge_deterministic_for_generated_inputs() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;

        for _ in 0..1024 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);

            let parser_layout = DocumentLayout {
                page_margin_top_twips: optional_i32(seed, 3),
                font_size_body_hp: optional_usize(seed, 7),
                body_first_line_indent_twips: optional_i32(seed, 11),
                list_label_width_twips: optional_i32(seed, 13),
                ..DocumentLayout::default()
            };
            let probe = LayoutProbeOutput {
                page_margin_top_twips: optional_i32(seed.rotate_left(5), 3),
                font_size_body_hp: optional_usize(seed.rotate_left(9), 7),
                body_first_line_indent_twips: optional_i32(seed.rotate_left(13), 11),
                list_label_width_twips: optional_i32(seed.rotate_left(17), 13),
                ..LayoutProbeOutput::default()
            };

            let once = merge_probe_and_parser_layout(&probe, parser_layout.clone());
            let twice = merge_probe_and_parser_layout(&probe, parser_layout);
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn pt_to_twips_monotonic_nonnegative_domain() {
        let mut previous = pt_to_twips_rounded(0.0).expect("0.0pt should convert");

        for step in 1..5000 {
            let points = step as f64 / 8.0;
            let current = pt_to_twips_rounded(points).expect("finite pt should convert");
            assert!(
                current >= previous,
                "pt->twips conversion must be monotonic: prev={previous}, current={current}, pt={points}"
            );
            previous = current;
        }
    }

    fn optional_i32(seed: u64, shift: u8) -> Option<i32> {
        let gate = ((seed >> shift) & 0b11) as u8;
        if gate == 0 {
            None
        } else {
            Some(((seed >> (shift + 5)) as i32 % 3000) - 1500)
        }
    }

    fn optional_usize(seed: u64, shift: u8) -> Option<usize> {
        let gate = ((seed >> shift) & 0b11) as u8;
        if gate == 0 {
            None
        } else {
            Some(((seed >> (shift + 7)) as usize % 40) + 10)
        }
    }
}
