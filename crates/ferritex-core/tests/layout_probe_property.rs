use ferritex_core::{
    layout_probe::merge_probe_and_parser_layout,
    model::{DocumentLayout, LayoutProbeOutput},
};

#[test]
fn property_merge_precedence_probe_then_parser() {
    let candidates_i32 = [None, Some(-100), Some(0), Some(240), Some(1500)];
    let candidates_usize = [None, Some(20), Some(24), Some(28), Some(36)];

    for parser_margin in candidates_i32 {
        for probe_margin in candidates_i32 {
            for parser_font in candidates_usize {
                for probe_font in candidates_usize {
                    let parser = DocumentLayout {
                        page_margin_left_twips: parser_margin,
                        font_size_body_hp: parser_font,
                        ..DocumentLayout::default()
                    };
                    let probe = LayoutProbeOutput {
                        page_margin_left_twips: probe_margin,
                        font_size_body_hp: probe_font,
                        ..LayoutProbeOutput::default()
                    };

                    let merged = merge_probe_and_parser_layout(&probe, parser.clone());

                    assert_eq!(
                        merged.page_margin_left_twips,
                        probe_margin.or(parser_margin),
                        "margin precedence mismatch: probe={probe_margin:?}, parser={parser_margin:?}"
                    );
                    assert_eq!(
                        merged.font_size_body_hp,
                        probe_font.or(parser_font),
                        "font precedence mismatch: probe={probe_font:?}, parser={parser_font:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn property_merge_is_idempotent_for_fixed_inputs() {
    let parser = DocumentLayout {
        page_margin_top_twips: Some(500),
        body_line_spacing_twips: Some(320),
        list_label_sep_twips: Some(120),
        ..DocumentLayout::default()
    };
    let probe = LayoutProbeOutput {
        page_margin_top_twips: Some(700),
        list_label_sep_twips: Some(140),
        ..LayoutProbeOutput::default()
    };

    let once = merge_probe_and_parser_layout(&probe, parser.clone());
    let twice = merge_probe_and_parser_layout(&probe, parser);
    assert_eq!(once, twice);
}
