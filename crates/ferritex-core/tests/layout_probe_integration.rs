use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ferritex_core::{
    layout_probe::merge_probe_and_parser_layout, model::LayoutProbeOutput,
    parser::latex::parse_latex_file,
};

#[test]
fn multi_file_merge_probe_overrides_parser_values() {
    let root = create_temp_dir("ferritex_layout_probe_merge");

    let style_tex = root.join("style.tex");
    let main_tex = root.join("main.tex");
    std::fs::write(
        &style_tex,
        "\\geometry{left=25mm,right=20mm,top=20mm,bottom=20mm}\n\\setlength{\\parindent}{1cm}\n",
    )
    .expect("failed to write style.tex");
    std::fs::write(
        &main_tex,
        "\\documentclass{article}\n\\usepackage{geometry}\n\\input{style}\n\\begin{document}\nBody.\\end{document}\n",
    )
    .expect("failed to write main.tex");

    let parsed = parse_latex_file(&main_tex).expect("parse_latex_file should succeed");
    assert_eq!(parsed.layout.page_margin_left_twips, Some(1417));
    assert_eq!(parsed.layout.body_first_line_indent_twips, Some(567));

    let probe = LayoutProbeOutput {
        page_margin_left_twips: Some(1800),
        body_first_line_indent_twips: Some(900),
        body_line_spacing_twips: Some(333),
        ..LayoutProbeOutput::default()
    };

    let merged = merge_probe_and_parser_layout(&probe, parsed.layout);

    assert_eq!(merged.page_margin_left_twips, Some(1800));
    assert_eq!(merged.body_first_line_indent_twips, Some(900));
    assert_eq!(merged.body_line_spacing_twips, Some(333));
    assert_eq!(merged.page_margin_right_twips, Some(1134));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn multi_file_merge_keeps_parser_values_when_probe_absent() {
    let root = create_temp_dir("ferritex_layout_probe_parser_fallback");

    let style_tex = root.join("style.tex");
    let main_tex = root.join("main.tex");
    std::fs::write(
        &style_tex,
        "\\setlength{\\parindent}{1.5cm}\n\\setlist{leftmargin=2em,labelsep=0.5em}\n",
    )
    .expect("failed to write style.tex");
    std::fs::write(
        &main_tex,
        "\\documentclass{article}\n\\usepackage{enumitem}\n\\input{style}\n\\begin{document}\n\\begin{itemize}\\item One\\end{itemize}\n\\end{document}\n",
    )
    .expect("failed to write main.tex");

    let parsed = parse_latex_file(&main_tex).expect("parse_latex_file should succeed");

    let parser_layout = parsed.layout.clone();
    let merged = merge_probe_and_parser_layout(&LayoutProbeOutput::default(), parsed.layout);

    assert_eq!(
        merged.page_margin_left_twips,
        parser_layout.page_margin_left_twips
    );
    assert_eq!(
        merged.body_first_line_indent_twips,
        parser_layout.body_first_line_indent_twips
    );
    assert_eq!(
        merged.list_left_indent_twips,
        parser_layout.list_left_indent_twips
    );
    assert_eq!(
        merged.list_label_sep_twips,
        parser_layout.list_label_sep_twips
    );

    let _ = std::fs::remove_dir_all(root);
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}
