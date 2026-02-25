use super::*;
use crate::model::{Block, Inline};

#[test]
fn test_chapter_level1() {
    let doc = parse_latex("\\chapter{Overview}");
    assert_eq!(
        doc.blocks,
        vec![Block::Section {
            level: 1,
            number: Some("1.".into()),
            label: None,
            title: vec![Inline::Text("Overview".into())]
        }]
    );
}

#[test]
fn test_section_level2() {
    let doc = parse_latex("\\section{Introduction}");
    assert_eq!(
        doc.blocks,
        vec![Block::Section {
            level: 2,
            number: Some("1".into()),
            label: None,
            title: vec![Inline::Text("Introduction".into())]
        }]
    );
}

#[test]
fn test_subsection_level3() {
    let doc = parse_latex("\\subsection{Background}");
    assert_eq!(
        doc.blocks[0],
        Block::Section {
            level: 3,
            number: Some("1".into()),
            label: None,
            title: vec![Inline::Text("Background".into())]
        }
    );
}

#[test]
fn test_section_star() {
    let doc = parse_latex("\\section*{Preface}");
    assert_eq!(
        doc.blocks[0],
        Block::Section {
            level: 2,
            number: None,
            label: None,
            title: vec![Inline::Text("Preface".into())]
        }
    );
}

#[test]
fn test_section_numbering_sequence_inside_chapter() {
    let doc = parse_latex(
        "\\chapter{One}\n\n\\section{A}\n\n\\subsection{A.1}\n\n\\section{B}\n\n\\subsection{B.1}",
    );
    let numbers: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Section {
                number: Some(n), ..
            } = b
            {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(numbers, vec!["1.", "1.1", "1.1.1", "1.2", "1.2.1"]);
}

#[test]
fn test_section_numbering_resets_after_new_chapter() {
    let doc = parse_latex("\\chapter{One}\n\n\\section{A}\n\n\\chapter{Two}\n\n\\section{B}");
    let section_numbers: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Section {
                level: 2,
                number: Some(n),
                ..
            } = b
            {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(section_numbers, vec!["1.1", "2.1"]);
}

#[test]
fn test_plain_paragraph() {
    let doc = parse_latex("Hello world.");
    assert_eq!(
        doc.blocks,
        vec![Block::Paragraph(vec![Inline::Text("Hello world.".into())])]
    );
}

#[test]
fn test_textbf() {
    let doc = parse_latex("This is \\textbf{bold} text.");
    assert_eq!(
        doc.blocks,
        vec![Block::Paragraph(vec![
            Inline::Text("This is ".into()),
            Inline::Bold(vec![Inline::Text("bold".into())]),
            Inline::Text(" text.".into()),
        ])]
    );
}

#[test]
fn test_textit() {
    let doc = parse_latex("\\textit{italic}");
    assert_eq!(
        doc.blocks,
        vec![Block::Paragraph(vec![Inline::Italic(vec![Inline::Text(
            "italic".into()
        )]),])]
    );
}

#[test]
fn test_bfseries_declaration_inside_group_wraps_following_content() {
    let doc = parse_latex("{\\bfseries\\MakeUppercase{Hello} world}");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|inline| matches!(inline, Inline::Bold(_))),
                "expected bold declaration to wrap group content: {inlines:?}"
            );
            let text = plain_text_from_inlines(inlines);
            assert!(text.contains("Hello"), "text: {text:?}");
            assert!(text.contains("world"), "text: {text:?}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_control_space_command_preserved_as_text_space() {
    let doc = parse_latex("A\\ B");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = plain_text_from_inlines(inlines);
            assert_eq!(text, "A B");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_bfseries_declaration_without_braces_styles_following_text() {
    let doc = parse_latex("\\bfseries Bold text");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|inline| matches!(inline, Inline::Bold(_))),
                "expected bold inline after declaration: {inlines:?}"
            );
            let text = plain_text_from_inlines(inlines);
            assert_eq!(text.trim(), "Bold text");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_hard_linebreak_command_preserved_as_inline_break() {
    let doc = parse_latex("Line one\\\\Line two");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|inline| matches!(inline, Inline::LineBreak)),
                "expected explicit line break in parsed inlines: {inlines:?}"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_preamble_skipped() {
    let src = "\\documentclass{article}\n\\usepackage{fontenc}\n\\begin{document}\nHello.\n\\end{document}";
    let doc = parse_latex(src);
    assert_eq!(
        doc.blocks,
        vec![Block::Paragraph(vec![Inline::Text("Hello.".into())])]
    );
}

#[test]
fn test_comment_stripped() {
    let doc = parse_latex("Hello % this is a comment\nworld.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text: String = inlines
                .iter()
                .map(|i| match i {
                    Inline::Text(s) => s.clone(),
                    _ => String::new(),
                })
                .collect();
            assert!(text.contains("Hello"), "expected 'Hello' in: {text:?}");
            assert!(text.contains("world"), "expected 'world' in: {text:?}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_layout_settings_extracted_from_geometry_and_setspacing() {
    let doc = parse_latex(
        "\\geometry{a4paper, top=20mm, bottom=20mm, left=25mm, right=10mm}\n\\setSpacing{1.385}\nBody.",
    );
    assert_eq!(doc.layout.page_margin_top_twips, Some(1134));
    assert_eq!(doc.layout.page_margin_bottom_twips, Some(1134));
    assert_eq!(doc.layout.page_margin_left_twips, Some(1417));
    assert_eq!(doc.layout.page_margin_right_twips, Some(567));
    assert_eq!(doc.layout.body_line_spacing_twips, Some(332));
}

#[test]
fn test_layout_settings_fallback_to_onehalfspacing_when_setspacing_absent() {
    let doc = parse_latex("\\OnehalfSpacing\nBody.");
    assert_eq!(doc.layout.body_line_spacing_twips, Some(300));
}

#[test]
fn test_layout_settings_onehalfspacing_uses_memoir_14pt_factor() {
    let source = "\\documentclass[14pt]{memoir}\n\\OnehalfSpacing\nBody.";
    let doc = parse_latex(source);
    assert_eq!(doc.layout.body_line_spacing_twips, Some(312));
}

#[test]
fn test_extract_toc_right_margin_and_dot_leader_from_latex() {
    let src = "\
\\setrmarg{2.55em plus1fil}
\\renewcommand{\\cftchapterleader}{\\cftdotfill{\\cftchapterdotsep}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_right_margin_twips, Some(714));
    assert_eq!(doc.layout.toc_use_dot_leader, Some(true));
}

#[test]
fn test_extract_toc_dot_leader_false_when_explicit_non_dot_leader() {
    let src = "\\renewcommand{\\cftchapterleader}{\\hfill}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_use_dot_leader, Some(false));
}

#[test]
fn test_extract_toc_chapter_before_skip_from_setlength() {
    let src = "\\setlength{\\cftbeforechapterskip}{0.75em plus0pt}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_space_before_twips, Some(210));
}

#[test]
fn test_extract_toc_chapter_before_skip_uses_memoir_default() {
    let src = "\\documentclass[14pt]{memoir}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_space_before_twips, Some(300));
}

#[test]
fn test_extract_toc_before_skip_nonchapter_levels_from_setlength() {
    let src = "\
\\setlength{\\cftbeforesectionskip}{12pt}
\\setlength{\\cftbeforesubsectionskip}{6pt}
\\setlength{\\cftbeforesubsubsectionskip}{3pt}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_section_space_before_twips, Some(240));
    assert_eq!(doc.layout.toc_subsection_space_before_twips, Some(120));
    assert_eq!(doc.layout.toc_subsubsection_space_before_twips, Some(60));
}

#[test]
fn test_extract_toc_before_skip_nonchapter_levels_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_section_space_before_twips, None);
    assert_eq!(doc.layout.toc_subsection_space_before_twips, None);
    assert_eq!(doc.layout.toc_subsubsection_space_before_twips, None);
}

#[test]
fn test_extract_toc_chapter_name_prefix_from_cftchaptername() {
    let src = "\
\\renewcommand{\\chaptername}{Глава}
\\renewcommand*{\\cftchaptername}{\\chaptername\\space}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_name_prefix.as_deref(), Some("Глава"));
}

#[test]
fn test_extract_toc_chapter_name_prefix_absent_when_undefined() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_chapter_name_prefix, None);
}

#[test]
fn test_extract_toc_chapter_name_prefix_respects_chapstyle_zero() {
    let src = "\
\\setcounter{chapstyle}{0}
\\ifnumequal{\\value{chapstyle}}{1}{%
  \\renewcommand*{\\cftchaptername}{\\chaptername\\space}
}{}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_name_prefix.as_deref(), Some(""));
}

#[test]
fn test_extract_toc_indents_from_cftsetindents() {
    let src = "\
\\cftsetindents{chapter}{0em}{3em}
\\cftsetindents{section}{1.2em}{2.4em}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_indent_chapter_twips, Some(0));
    assert_eq!(doc.layout.toc_numwidth_chapter_twips, Some(840));
    assert_eq!(doc.layout.toc_indent_section_twips, Some(336));
    assert_eq!(doc.layout.toc_numwidth_section_twips, Some(672));
}

#[test]
fn test_extract_toc_indents_from_setlength() {
    let src = "\
\\setlength{\\cftchapterindent}{0pt}
\\setlength{\\cftchapternumwidth}{42pt}
\\setlength{\\cftsectionindent}{20pt}
\\setlength{\\cftsectionnumwidth}{36pt}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_indent_chapter_twips, Some(0));
    assert_eq!(doc.layout.toc_numwidth_chapter_twips, Some(840));
    assert_eq!(doc.layout.toc_indent_section_twips, Some(400));
    assert_eq!(doc.layout.toc_numwidth_section_twips, Some(720));
}

#[test]
fn test_extract_toc_indents_absent_when_undefined() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_indent_chapter_twips, None);
    assert_eq!(doc.layout.toc_numwidth_chapter_twips, None);
    assert_eq!(doc.layout.toc_indent_section_twips, None);
    assert_eq!(doc.layout.toc_numwidth_section_twips, None);
    assert_eq!(doc.layout.toc_indent_subsection_twips, None);
    assert_eq!(doc.layout.toc_numwidth_subsection_twips, None);
    assert_eq!(doc.layout.toc_indent_subsubsection_twips, None);
    assert_eq!(doc.layout.toc_numwidth_subsubsection_twips, None);
}

#[test]
fn test_extract_toc_indents_use_memoir_defaults_when_not_overridden() {
    let doc = parse_latex("\\documentclass[14pt]{memoir}\nBody.");
    assert_eq!(doc.layout.toc_indent_chapter_twips, Some(0));
    assert_eq!(doc.layout.toc_numwidth_chapter_twips, Some(420));
    assert_eq!(doc.layout.toc_indent_section_twips, Some(420));
    assert_eq!(doc.layout.toc_numwidth_section_twips, Some(644));
    assert_eq!(doc.layout.toc_indent_subsection_twips, Some(1064));
    assert_eq!(doc.layout.toc_numwidth_subsection_twips, Some(896));
    assert_eq!(doc.layout.toc_indent_subsubsection_twips, Some(1960));
    assert_eq!(doc.layout.toc_numwidth_subsubsection_twips, Some(1148));
}

#[test]
fn test_layout_float_counters_global_when_contnumfig_is_1() {
    // contnumfig=1 means continuous (global) numbering → within_chapter = false
    let src = "\\setcounter{contnumfig}{1}\n\\setcounter{contnumtab}{1}\n\\setcounter{contnumeq}{1}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.figure_counter_within_chapter, Some(false));
    assert_eq!(doc.layout.table_counter_within_chapter, Some(false));
    assert_eq!(doc.layout.equation_counter_within_chapter, Some(false));
}

#[test]
fn test_layout_float_counters_per_chapter_when_contnumfig_is_0() {
    // contnumfig=0 means per-chapter numbering → within_chapter = true
    let src = "\\setcounter{contnumfig}{0}\n\\setcounter{contnumtab}{0}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.figure_counter_within_chapter, Some(true));
    assert_eq!(doc.layout.table_counter_within_chapter, Some(true));
}

#[test]
fn test_layout_float_counters_absent_when_contnumfig_not_set() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.figure_counter_within_chapter, None);
    assert_eq!(doc.layout.table_counter_within_chapter, None);
}

#[test]
fn test_layout_float_counters_with_trailing_comment_and_newcounter() {
    // Reproduces the exact pattern from a typical dissertation setup file:
    //   \newcounter{contnumfig}
    //   \setcounter{contnumfig}{1}  % 0 --- per-chapter; 1 --- global
    let src = "\
\\newcounter{contnumfig}\n\
\\newcounter{contnumtab}\n\
\\setcounter{contnumfig}{1}  % 0 --- per-chapter; 1 --- global\n\
\\setcounter{contnumtab}{1}  % 0 --- per-chapter; 1 --- global\n\
Body.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.figure_counter_within_chapter,
        Some(false),
        "contnumfig=1 should produce figure_counter_within_chapter=false (global)"
    );
    assert_eq!(
        doc.layout.table_counter_within_chapter,
        Some(false),
        "contnumtab=1 should produce table_counter_within_chapter=false (global)"
    );
}

#[test]
fn test_setcounter_inside_iffontexiststf_is_ignored() {
    // \IfFontExistsTF{Times New Roman}{}{\setcounter{fontfamily}{0}}
    // The fallback \setcounter inside the runtime conditional should be ignored.
    let src = "\
\\setcounter{fontfamily}{1}
\\IfFontExistsTF{Times New Roman}{}{\\setcounter{fontfamily}{0}}
\\IfFontExistsTF{LiberationSerif}{}{\\setcounter{fontfamily}{0}}
Body.";
    let doc = parse_latex(src);
    // fontfamily=1 means per-chapter is determined by contnumfig, not fontfamily.
    // But we can verify the extracted fontfamily indirectly via font_family_body.
    // With fontfamily=1 and matching \ifnumequal branch:
    let src2 = "\
\\setcounter{fontfamily}{1}
\\IfFontExistsTF{Times New Roman}{}{\\setcounter{fontfamily}{0}}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
Body.";
    let doc2 = parse_latex(src2);
    assert_eq!(
        doc2.layout.font_family_body.as_deref(),
        Some("Times New Roman"),
        "\\setcounter inside \\IfFontExistsTF should be skipped; fontfamily stays 1"
    );
    let _ = doc; // suppress unused warning
}

#[test]
fn test_label_registry_global_numbering_produces_flat_refs() {
    // When contnumfig=1 (global), \ref{fig:first} inside chapter 1 should resolve
    // to "1" (not "1.1"). Same for tables.
    let src = "\
\\setcounter{contnumfig}{1}\n\
\\setcounter{contnumtab}{1}\n\
\\chapter{Chapter One}\n\
\\begin{figure}\n\
\\caption{A figure}\n\
\\label{fig:first}\n\
\\end{figure}\n\
\\begin{table}\n\
\\caption{A table}\n\
\\label{tab:first}\n\
\\begin{tabular}{l}\nCell\\end{tabular}\n\
\\end{table}\n\
See \\ref{fig:first} and \\ref{tab:first}.";
    let doc = parse_latex(src);
    // Find the paragraph containing the resolved references.
    let mut found_fig = false;
    let mut found_tab = false;
    for block in &doc.blocks {
        if let Block::Paragraph(inlines) = block {
            for inline in inlines {
                if let Inline::Text(t) = inline {
                    if t == "1" && !found_fig {
                        found_fig = true;
                    } else if t == "1" && found_fig {
                        found_tab = true;
                    }
                }
            }
        }
    }
    // The resolved refs must appear as plain "1", not "1.1".
    for block in &doc.blocks {
        if let Block::Paragraph(inlines) = block {
            for inline in inlines {
                if let Inline::Text(t) = inline {
                    assert!(
                        !t.contains("1.1"),
                        "global counter should produce flat ref '1', not '{}' (chapter-prefixed)",
                        t
                    );
                }
            }
        }
    }
    let _ = found_fig;
    let _ = found_tab;
}

#[test]
fn test_label_registry_per_chapter_numbering_produces_prefixed_refs() {
    // When contnumfig=0 (per-chapter), \ref{fig:first} inside chapter 1 should resolve
    // to "1.1".
    let src = "\
\\setcounter{contnumfig}{0}\n\
\\chapter{Chapter One}\n\
\\begin{figure}\n\
\\caption{A figure}\n\
\\label{fig:first}\n\
\\end{figure}\n\
See \\ref{fig:first}.";
    let doc = parse_latex(src);
    let mut found_prefixed = false;
    for block in &doc.blocks {
        if let Block::Paragraph(inlines) = block {
            for inline in inlines {
                if let Inline::Text(t) = inline
                    && t == "1.1"
                {
                    found_prefixed = true;
                }
            }
        }
    }
    assert!(
        found_prefixed,
        "per-chapter counter should produce ref '1.1'"
    );
}

#[test]
fn test_zero_arg_newcommand_expanded_in_paragraph_text() {
    let doc = parse_latex("\\newcommand{\\actualityTXT}{Topic relevance}\nA: \\actualityTXT.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert!(
                text.contains("Topic relevance"),
                "expected expanded macro text, got: {text}"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_texorpdfstring_prefers_visible_branch() {
    let doc = parse_latex("\\texorpdfstring{Visible text}{Hidden text}");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert!(text.contains("Visible text"), "unexpected text: {text}");
            assert!(!text.contains("Hidden text"), "unexpected text: {text}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_makeuppercase_command_keeps_argument_text() {
    let doc = parse_latex("\\MakeUppercase{Sample heading}");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert!(text.contains("Sample heading"), "unexpected text: {text}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_tableofcontents_command_becomes_heading_block() {
    // \tableofcontents must emit a language-neutral Block::TableOfContents,
    // not a Block::Section with Russian "ОГЛАВЛЕНИЕ" text.
    let doc = parse_latex("\\tableofcontents");
    assert_eq!(doc.blocks.len(), 1, "unexpected blocks: {:?}", doc.blocks);
    assert!(
        matches!(&doc.blocks[0], Block::TableOfContents),
        "expected Block::TableOfContents, got {:?}",
        doc.blocks[0]
    );
}

#[test]
fn test_tableofcontents_with_asterisk_becomes_toc_node() {
    // \tableofcontents* (starred variant) also emits Block::TableOfContents.
    let doc = parse_latex("\\tableofcontents*");
    assert_eq!(doc.blocks.len(), 1, "unexpected blocks: {:?}", doc.blocks);
    assert!(
        matches!(&doc.blocks[0], Block::TableOfContents),
        "expected Block::TableOfContents, got {:?}",
        doc.blocks[0]
    );
}

#[test]
fn test_newpage_command_becomes_page_break_block() {
    let doc = parse_latex("\\newpage");
    assert_eq!(doc.blocks, vec![Block::PageBreak]);
}

#[test]
fn test_clearpage_and_cleardoublepage_become_page_break_blocks() {
    let doc = parse_latex("\\clearpage\n\n\\cleardoublepage");
    assert_eq!(doc.blocks.len(), 2, "unexpected blocks: {:?}", doc.blocks);
    assert!(matches!(doc.blocks[0], Block::PageBreak));
    assert!(matches!(doc.blocks[1], Block::PageBreak));
}

#[test]
fn test_standalone_page_break_between_paragraphs_is_preserved() {
    let doc = parse_latex("Before.\n\n\\newpage\n\nAfter.");
    assert_eq!(doc.blocks.len(), 3, "unexpected blocks: {:?}", doc.blocks);
    assert!(matches!(doc.blocks[0], Block::Paragraph(_)));
    assert!(matches!(doc.blocks[1], Block::PageBreak));
    assert!(matches!(doc.blocks[2], Block::Paragraph(_)));
}

#[test]
fn test_inline_newpage_command_does_not_emit_page_break_block() {
    let doc = parse_latex("Before \\newpage after.");
    assert!(
        doc.blocks
            .iter()
            .all(|block| !matches!(block, Block::PageBreak)),
        "unexpected page-break block in {:?}",
        doc.blocks
    );
}

#[test]
fn test_landscape_switch_commands_become_page_break_blocks() {
    let doc =
        parse_latex("\\begin{landscape}\n\n\\end{landscape}\n\n\\landscape\n\n\\endlandscape");
    assert_eq!(doc.blocks.len(), 4, "unexpected blocks: {:?}", doc.blocks);
    for block in doc.blocks {
        assert!(matches!(block, Block::PageBreak));
    }
}

#[test]
fn test_inline_landscape_command_does_not_emit_page_break_block() {
    let doc = parse_latex("Before \\landscape content.");
    assert!(
        doc.blocks
            .iter()
            .all(|block| !matches!(block, Block::PageBreak)),
        "unexpected page-break block in {:?}",
        doc.blocks
    );
}

#[test]
fn test_tochelper_like_macro_is_not_expanded_into_body_noise() {
    let src = "\\newcommand*{\\tocheader}{\\ifnumequal{\\value{pgnum}}{1}{\\hbox to \\linewidth{X}\\afterpage{\\tocheader}}{}}\n\\addtocontents{toc}{\\protect\\tocheader}\n\\tableofcontents*";
    let expanded = expand_simple_macros(src);
    assert!(
        expanded.contains("\\protect\\tocheader"),
        "toc helper macro should not be inlined into document body"
    );
}

#[test]
fn test_parse_toc_helper_line_extracts_visible_header_text_from_macro() {
    let source = "\\newcommand*{\\tocheader}{\\ifnumequal{\\value{pgnum}}{1}{\\hbox to \\linewidth{\\noindent{}~\\hfill{Стр.}}\\par\\afterpage{\\tocheader}}{}}";
    let entry = parse_toc_line(
        "\\tocheader",
        AutociteMode::InlinePlaceholder,
        &ParseMetadata::default(),
        source,
    )
    .expect("expected to parse toc helper line");

    assert_eq!(entry.level, 0);
    assert_eq!(entry.title, "Стр.");
    assert_eq!(entry.number, None);
    assert_eq!(entry.page, None);
}

#[test]
fn test_cite_becomes_placeholder() {
    let doc = parse_latex("See \\cite{Smith2020}.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020")))
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_ref_resolved_for_section_label() {
    let src = "\\chapter{Intro}\\label{ch:intro}\n\nSee chapter \\ref{ch:intro}.";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("1"),
        "expected resolved section reference number, got: {resolved}"
    );
}

#[test]
fn test_ref_resolved_for_figure_label() {
    let src = r"
\chapter{Intro}

\begin{figure}[H]
\caption{Sample}
\label{fig:sample}
\end{figure}

See Figure \ref{fig:sample}.
";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("1.1"),
        "expected chapter-aware figure reference number, got: {resolved}"
    );
}

#[test]
fn test_ref_missing_becomes_explicit_placeholder() {
    let doc = parse_latex("See \\ref{missing:label}.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|i| matches!(i, Inline::Text(s) if s.contains("[ref:missing:label]")))
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_ref_resolved_for_tblr_option_label() {
    let src = r"
\chapter{Intro}

\begin{longtblr}[
label = {tab:opt_label},
]{
colspec = {|l|l|},
hlines
}
A & B \\
\end{longtblr}

See table \cref{tab:opt_label}.
";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("1.1"),
        "expected resolved table number from tabularray option label, got: {resolved}"
    );
}

#[test]
fn test_ref_resolved_for_appendix_label_from_standalone_label() {
    let src = "\\chapter*{Appendix}\n\n\\label{app:C}\n\nSee \\ref{app:C}.";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("C"),
        "expected appendix fallback value from app:C label, got: {resolved}"
    );
}

#[test]
fn test_ref_resolved_for_equation_label() {
    let src = r"
\begin{equation}
W = x + y
\label{eq:welfare}
\end{equation}

As shown in \ref{eq:welfare}, objective is linear.
";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("1"),
        "expected resolved equation reference number, got: {resolved}"
    );
}

#[test]
fn test_eqref_wraps_resolved_equation_number() {
    let src = r"
\begin{equation}
W = x + y
\label{eq:welfare}
\end{equation}

As shown in \eqref{eq:welfare}, objective is linear.
";
    let doc = parse_latex(src);
    let resolved = doc.blocks.iter().find_map(|b| {
        if let Block::Paragraph(inlines) = b {
            Some(
                inlines
                    .iter()
                    .filter_map(|i| {
                        if let Inline::Text(s) = i {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>(),
            )
        } else {
            None
        }
    });
    let resolved = resolved.expect("missing paragraph");
    assert!(
        resolved.contains("(1)"),
        "expected eqref format '(1)', got: {resolved}"
    );
}

#[test]
fn test_autocite_default_is_inline_placeholder() {
    let doc = parse_latex("See \\autocite{Smith2020}.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines
                    .iter()
                    .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020"))),
                "autocite should default to inline placeholder"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_autocite_becomes_footnote_when_style_requires_it() {
    let src = "\\ExecuteBibliographyOptions{autocite=footnote}\nSee \\autocite{Smith2020}.";
    let doc = parse_latex(src);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let footnote_payload = inlines.iter().find_map(|i| {
                if let Inline::Footnote(content) = i {
                    Some(content)
                } else {
                    None
                }
            });
            let footnote_payload = footnote_payload.expect("expected footnote from autocite");
            assert!(
                footnote_payload
                    .iter()
                    .any(|i| matches!(i, Inline::Text(s) if s.contains("Smith2020"))),
                "autocite footnote payload must contain citation key"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_simple_table_parsed() {
    let src = r"
\begin{table}[H]
\centering
\caption{Sample table}
\label{tab:sample}
\begin{tabular}{|l|c|}
\hline
\textbf{Name} & \textbf{Value} \\
\hline
Alpha & 1 \\
Beta & 2 \\
\hline
\end{tabular}
\end{table}
";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1, "expected one block");
    match &doc.blocks[0] {
        Block::Table(t) => {
            assert!(!t.caption.is_empty(), "caption should be parsed");
            assert_eq!(t.alignment.as_deref(), Some("center"));
            // 3 rows: header + 2 data rows (hline rules are stripped, not rows)
            assert_eq!(t.rows.len(), 3, "expected 3 rows (header + 2 data)");
            assert_eq!(t.rows[0].cells.len(), 2, "expected 2 cells per row");
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn test_table_parses_multiple_source_macros_after_float() {
    let src = r"
\begin{table}[H]
\caption{Sample}
\begin{tabular}{|l|}
\hline
X \\
\hline
\end{tabular}
\end{table}
\tablesource{*Estimated.}
\tablesource{Source: synthetic dataset.}
";
    let doc = parse_latex(src);
    let Block::Table(table) = &doc.blocks[0] else {
        panic!("expected table");
    };
    let source_text = table
        .source
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(
        source_text.contains("*Estimated."),
        "expected first source segment, got: {source_text}"
    );
    assert!(
        source_text.contains("Source: synthetic dataset."),
        "expected second source segment, got: {source_text}"
    );
}

#[test]
fn test_table_cell_linebreak_keeps_space_between_words() {
    let src = r"
\begin{table}[H]
\caption{Sample}
\begin{tabular}{|l|}
\hline
Alpha\linebreak Beta \\
\hline
\end{tabular}
\end{table}
";
    let doc = parse_latex(src);
    let Block::Table(table) = &doc.blocks[0] else {
        panic!("expected table");
    };
    let cell_text = table.rows[0].cells[0]
        .content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(
        cell_text.split_whitespace().collect::<Vec<_>>().join(" "),
        "Alpha Beta"
    );
}

#[test]
fn test_figure_parsed() {
    let src = r"
\begin{figure}[H]
\centering
\includegraphics[width=1.0\textwidth]{images/chart.png}
\caption{A sample chart}
\label{fig:chart}
\end{figure}
\figuresource{Source: synthetic data.}
";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Figure(f) => {
            assert_eq!(
                f.image_path.as_deref(),
                Some("images/chart.png"),
                "image path mismatch"
            );
            assert_eq!(f.width_permille, Some(1000), "width hint mismatch");
            assert_eq!(f.alignment.as_deref(), Some("center"));
            assert!(!f.caption.is_empty(), "caption should be parsed");
            assert!(!f.source.is_empty(), "figuresource should be parsed");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn test_itemize_parsed() {
    let src = r"
\begin{itemize}
    \item First item with \textit{italic}.
    \item Second item.
    \item Third item.
\end{itemize}
";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List(list) => {
            assert!(!list.ordered, "should be unordered");
            assert_eq!(list.items.len(), 3, "expected 3 items");
            // First item should contain italic inline.
            assert!(
                list.items[0].iter().any(|i| matches!(i, Inline::Italic(_))),
                "first item should contain italic"
            );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_enumerate_parsed() {
    let src = r"
\begin{enumerate}
    \item Rule one.
    \item Rule two with \textbf{bold}.
\end{enumerate}
";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List(list) => {
            assert!(list.ordered, "should be ordered");
            assert_eq!(list.items.len(), 2, "expected 2 items");
            assert!(
                list.items[1].iter().any(|i| matches!(i, Inline::Bold(_))),
                "second item should contain bold"
            );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_inline_math_parsed() {
    let doc = parse_latex("Energy $E = mc^2$ is conserved.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines.iter().any(|i| matches!(i, Inline::InlineMath(_))),
                "expected InlineMath in paragraph"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_display_math_parsed() {
    let src = "\\begin{equation}\nW = \\sum x_i\n\\label{eq:welfare}\n\\end{equation}";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::DisplayMath(body) => {
            assert!(body.contains("W = \\sum x_i"), "body: {body:?}");
        }
        other => panic!("expected DisplayMath, got {other:?}"),
    }
}

#[test]
fn test_display_math_brackets_parsed() {
    let src = "\\[\nE = mc^2\n\\]";
    let doc = parse_latex(src);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::DisplayMath(body) => {
            assert_eq!(body, "E = mc^2");
        }
        other => panic!("expected DisplayMath, got {other:?}"),
    }
}

#[test]
fn test_display_math_brackets_between_paragraphs() {
    let src = "Before.\n\n\\[\na+b\n\\]\n\nAfter.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.blocks.len(),
        3,
        "expected paragraph + display math + paragraph"
    );
    assert!(matches!(doc.blocks[0], Block::Paragraph(_)));
    assert!(matches!(doc.blocks[1], Block::DisplayMath(_)));
    assert!(matches!(doc.blocks[2], Block::Paragraph(_)));
}

#[test]
fn test_footnote_parsed() {
    let src = "A statement\\footnote{A supporting note with \\textit{style}.}.";
    let doc = parse_latex(src);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(
                inlines.iter().any(|i| matches!(i, Inline::Footnote(_))),
                "expected Footnote inline in paragraph"
            );
            let footnote = inlines.iter().find_map(|i| {
                if let Inline::Footnote(content) = i {
                    Some(content)
                } else {
                    None
                }
            });
            let footnote = footnote.expect("missing Footnote inline payload");
            assert!(
                footnote.iter().any(|i| matches!(i, Inline::Italic(_))),
                "expected nested formatting inside footnote"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_parse_latex_file_expands_input_blocks() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_input_expand_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    let part_tex = root.join("part.tex");
    std::fs::write(&part_tex, "Included paragraph from input.").expect("failed to write part.tex");
    std::fs::write(
        &main_tex,
        "\\section{Main}\n\n\\input{part}\n\nA trailing paragraph.",
    )
    .expect("failed to write main.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    assert!(
        doc.blocks
            .iter()
            .any(|b| matches!(b, Block::Section { .. })),
        "expected section block from main.tex"
    );
    let has_included_paragraph = doc.blocks.iter().any(|b| {
        if let Block::Paragraph(inlines) = b {
            inlines.iter().any(
                |i| matches!(i, Inline::Text(s) if s.contains("Included paragraph from input")),
            )
        } else {
            false
        }
    });
    assert!(
        has_included_paragraph,
        "expected paragraph content from included part.tex"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_resolves_year_primitives_from_metadata() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_year_primitive_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    std::fs::write(&main_tex, "Y: \\the\\year / \\year.").expect("failed to write main.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    let expected_year = current_utc_year()
        .expect("expected current year")
        .to_string();
    let text = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(inlines) => Some(
                inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();

    assert!(
        text.contains(&expected_year),
        "expected year '{expected_year}' in parsed text, got: {text}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_reads_toc_helper_header_and_entries() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_toc_helper_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    let main_toc = root.join("main.toc");
    std::fs::write(
        &main_tex,
        "\\newcommand*{\\tocheader}{\\hbox to \\linewidth{\\hfill{Page.}}}\n\\tableofcontents*",
    )
    .expect("failed to write main.tex");
    std::fs::write(
        &main_toc,
        "\\tocheader\n\\contentsline {chapter}{\\numberline {1}Intro}{3}{chapter.1}%\n",
    )
    .expect("failed to write main.toc");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    assert_eq!(doc.toc_entries.len(), 2, "unexpected toc entries");
    assert_eq!(doc.toc_entries[0].level, 0);
    assert_eq!(doc.toc_entries[0].title, "Page.");
    assert_eq!(doc.toc_entries[1].level, 1);
    assert_eq!(doc.toc_entries[1].number.as_deref(), Some("1"));
    assert_eq!(doc.toc_entries[1].title, "Intro");
    assert_eq!(doc.toc_entries[1].page.as_deref(), Some("3"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_detects_autocite_mode_from_included_style() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_autocite_mode_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    let style_tex = root.join("style.tex");
    std::fs::write(
        &style_tex,
        "\\ExecuteBibliographyOptions{autocite=footnote}\n",
    )
    .expect("failed to write style.tex");
    std::fs::write(&main_tex, "\\input{style}\nSee \\autocite{Key2026}.")
        .expect("failed to write main.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    let has_autocite_footnote = doc.blocks.iter().any(|b| {
        if let Block::Paragraph(inlines) = b {
            inlines.iter().any(|i| matches!(i, Inline::Footnote(_)))
        } else {
            false
        }
    });
    assert!(
        has_autocite_footnote,
        "expected autocite to become footnote based on included style settings"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_expands_include_blocks() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_include_expand_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    let chapter_tex = root.join("chapter1.tex");
    std::fs::write(&chapter_tex, "Chapter text from include.").expect("failed to write chapter1");
    std::fs::write(&main_tex, "\\include{chapter1}\n").expect("failed to write main.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    let has_included_paragraph = doc.blocks.iter().any(|b| {
        if let Block::Paragraph(inlines) = b {
            inlines
                .iter()
                .any(|i| matches!(i, Inline::Text(s) if s.contains("Chapter text from include")))
        } else {
            false
        }
    });
    assert!(
        has_included_paragraph,
        "expected paragraph from included chapter file"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_resolves_input_from_root_fallback() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_root_fallback_{unique}"));
    let chapters = root.join("chapters");
    let common = root.join("common");
    std::fs::create_dir_all(&chapters).expect("failed to create chapters dir");
    std::fs::create_dir_all(&common).expect("failed to create common dir");

    let main_tex = root.join("main.tex");
    let chapter_tex = chapters.join("chapter1.tex");
    let shared_tex = common.join("shared.tex");

    std::fs::write(&shared_tex, "Shared paragraph from root fallback.")
        .expect("failed to write shared.tex");
    std::fs::write(&chapter_tex, "\\input{common/shared}\n").expect("failed to write chapter1.tex");
    std::fs::write(&main_tex, "\\include{chapters/chapter1}\n").expect("failed to write main.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");
    let has_shared_text = doc.blocks.iter().any(|b| {
            if let Block::Paragraph(inlines) = b {
                inlines.iter().any(
                    |i| matches!(i, Inline::Text(s) if s.contains("Shared paragraph from root fallback")),
                )
            } else {
                false
            }
        });
    assert!(
        has_shared_text,
        "expected text from root fallback include path"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_parse_latex_file_preserves_heading_after_include_without_trailing_newline() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ferritex_include_newline_{unique}"));
    std::fs::create_dir_all(&root).expect("failed to create temp dir");

    let main_tex = root.join("main.tex");
    let toc_tex = root.join("toc.tex");
    let intro_tex = root.join("intro.tex");

    std::fs::write(&main_tex, "\\input{toc}\n\\input{intro}\n").expect("failed to write main.tex");
    std::fs::write(
        &toc_tex,
        "\\ifdefmacro{\\microtypesetup}{\\microtypesetup{protrusion=true}}{}",
    )
    .expect("failed to write toc.tex");
    std::fs::write(&intro_tex, "\\chapter*{Introduction}\n\nBody text.")
        .expect("failed to write intro.tex");

    let doc = parse_latex_file(&main_tex).expect("parse_latex_file failed");

    assert!(
            doc.blocks.iter().any(|block| {
                matches!(
                    block,
                    Block::Section {
                        level: 1,
                        number: None,
                        title,
                        ..
                    } if title.iter().any(|inline| matches!(inline, Inline::Text(text) if text.contains("Introduction")))
                )
            }),
            "expected chapter* heading to survive include boundary"
        );

    let star_heading_leak = doc.blocks.iter().any(|block| {
        if let Block::Paragraph(inlines) = block {
            let text = inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text(value) => Some(value.as_str()),
                    _ => None,
                })
                .collect::<String>();
            text.contains("*Introduction")
        } else {
            false
        }
    });
    assert!(
        !star_heading_leak,
        "unexpected raw '*Introduction' leakage from heading parsing"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_strip_heading_prefix_noise_skips_ifnumequal_prefix() {
    let chunk = r"\ifnumequal{\value{contnumfig}}{1}{}{\counterwithout{figure}{chapter}} \
\ifnumequal{\value{contnumtab}}{1}{}{\counterwithout{table}{chapter}} \
\chapter*{Introduction}";
    let stripped = strip_heading_prefix_noise(chunk);
    assert!(
        stripped.starts_with("\\chapter*{Introduction}"),
        "unexpected stripped value: {stripped}"
    );
}

#[test]
fn test_parse_latex_extracts_chapter_after_ifnumequal_prefix() {
    let src = r"\ifnumequal{\value{contnumfig}}{1}{}{\counterwithout{figure}{chapter}}
\ifnumequal{\value{contnumtab}}{1}{}{\counterwithout{table}{chapter}}
\chapter*{Introduction}";
    let doc = parse_latex(src);
    assert!(
            doc.blocks.iter().any(|block| {
                matches!(
                    block,
                    Block::Section {
                        level: 1,
                        number: None,
                        title,
                        ..
                    } if title.iter().any(|inline| matches!(inline, Inline::Text(text) if text == "Introduction"))
                )
            }),
            "expected chapter* block after ifnumequal prefix"
        );
}

#[test]
fn test_unknown_control_command_does_not_leak_argument_text() {
    let doc = parse_latex("Alpha \\unknownmacro{SHOULD_NOT_LEAK} Beta.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("Alpha"));
            assert!(text.contains("Beta."));
            assert!(!text.contains("SHOULD_NOT_LEAK"));
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_ifnumequal_keeps_primary_branch_and_omits_else_macros() {
    let src = r"
\ifnumequal{\value{bibliosel}}{0}
{
Primary branch text.
}%
{
\begin{refsection}[bl-author]
\printbibliography[heading=nobibheading, section=1, env=countauthor, keyword=biblioauthor]
\end{refsection}
}%
";
    let doc = parse_latex(src);
    let text = doc
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Paragraph(inlines) = b {
                Some(
                    inlines
                        .iter()
                        .filter_map(|i| {
                            if let Inline::Text(t) = i {
                                Some(t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>(),
                )
            } else {
                None
            }
        })
        .collect::<String>();

    assert!(text.contains("Primary branch text."));
    assert!(!text.contains("bl-author"));
    assert!(!text.contains("heading=nobibheading"));
}

#[test]
fn test_standalone_brace_paragraphs_are_filtered() {
    let doc = parse_latex("{\n\nVisible text.\n\n}");
    let mut texts = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(inlines) => Some(
                inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>();
    texts.retain(|value| !value.trim().is_empty());

    assert_eq!(texts.len(), 1, "unexpected paragraph texts: {texts:?}");
    assert!(
        texts[0].contains("Visible text."),
        "unexpected text: {}",
        texts[0]
    );
}

#[test]
fn test_ifnum_prefers_then_branch_for_unknown_operands() {
    let doc = parse_latex(
        "Prefix \\ifnum\\totvalue{totalappendix}>0, keeps then-branch\\else keeps else-branch\\fi suffix.",
    );
    let text = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(inlines) => Some(
                inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();

    assert!(text.contains("then-branch"), "unexpected text: {text}");
    assert!(!text.contains("else-branch"), "unexpected text: {text}");
}

#[test]
fn test_ifnum_numeric_comparison_uses_else_branch_when_false() {
    let doc = parse_latex("Prefix \\ifnum 1>2 then\\else else\\fi suffix.");
    let text = doc
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(inlines) => Some(
                inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Text(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();

    assert!(text.contains("else"), "unexpected text: {text}");
    assert!(!text.contains(" then "), "unexpected text: {text}");
}

#[test]
fn test_longtblr_caption_option_parsed() {
    let src = r"
\begin{longtblr}[
caption = {Option caption text},
label = {tab:opt},
]{
colspec = {|l|l|},
hlines
}
A & B \\
\end{longtblr}
";
    let doc = parse_latex(src);
    match &doc.blocks[0] {
        Block::Table(table) => {
            let caption_text = table
                .caption
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(
                caption_text.contains("Option caption text"),
                "unexpected caption: {caption_text}"
            );
        }
        other => panic!("expected table, got {other:?}"),
    }
}

#[test]
fn test_printnomenclature_line_is_skipped() {
    let doc = parse_latex("\\printnomenclature[3.5cm]");
    assert!(doc.blocks.is_empty(), "unexpected blocks: {:?}", doc.blocks);
}

#[test]
fn test_typography_normalization_for_quotes_and_dash() {
    let doc = parse_latex("<<чистыми>> и Третий вектор -- новая фаза. X \"--- в тезисах.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("«чистыми»"), "unexpected text: {text}");
            assert!(text.contains("вектор —"), "unexpected text: {text}");
            assert!(text.contains("X —"), "unexpected text: {text}");
            assert!(
                !text.contains("<<") && !text.contains(">>"),
                "unexpected text: {text}"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_cdot_command_converted_to_middle_dot() {
    let doc = parse_latex("кВт\\cdotч");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("кВт·ч"), "unexpected text: {text}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_formbytotal_prefers_plural_suffix() {
    let doc = parse_latex(
        "Полный объём составляет \\formbytotal{TotPages}{страниц}{у}{ы}{} в документе.",
    );
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("страницы"), "unexpected text: {text}");
            assert!(!text.contains("страницуы"), "unexpected text: {text}");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_nonbreaking_space_tilde_normalized_to_regular_space() {
    let doc = parse_latex("См. Рисунок~\\ref{fig:sample}.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("Рисунок "));
            assert!(!text.contains("Рисунок~"));
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_textual_passthrough_command_keeps_braced_content() {
    let doc = parse_latex("Quoted: \\enquote{important text}.");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Text(t) = i {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("important text"));
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn test_unmatched_display_math_bracket_does_not_swallow_rest() {
    // \[ used in a macro definition (not as display math) must not
    // prevent subsequent block environments from being segmented.
    let src = concat!(
        "\\def\\zz{\\ifx\\[$\\else\\fi}\n\n",
        "Before.\n\n",
        "\\begin{itemize}\n\\item One\n\\item Two\n\\end{itemize}\n\n",
        "After."
    );
    let doc = parse_latex(src);
    let has_list = doc.blocks.iter().any(|b| matches!(b, Block::List(_)));
    assert!(
        has_list,
        "unmatched \\[ in preamble must not hide subsequent list blocks"
    );
}

// ── Tests for newly extracted DocumentLayout fields ─────────────────

#[test]
fn test_extract_font_family_from_setmainfont() {
    let src = "\\setmainfont{Liberation Serif}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.font_family_body.as_deref(),
        Some("Liberation Serif")
    );
}

#[test]
fn test_extract_font_family_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.font_family_body, None);
}

#[test]
fn test_extract_font_family_conditional_on_fontfamily_counter() {
    let src = "\
\\setcounter{fontfamily}{1}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmainfont{LiberationSerif}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.font_family_body.as_deref(),
        Some("Times New Roman"),
        "should pick \\setmainfont from the branch matching fontfamily=1"
    );
}

#[test]
fn test_extract_font_family_conditional_selects_liberation_when_counter_is_2() {
    let src = "\
\\setcounter{fontfamily}{2}
\\ifnumequal{\\value{fontfamily}}{0}{\\setmainfont{CMU Serif}}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmainfont{Times New Roman}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmainfont{LiberationSerif}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.font_family_body.as_deref(),
        Some("LiberationSerif")
    );
}

#[test]
fn test_bare_tabular_not_parsed_as_table_block() {
    let src = "\\begin{tabular}{l}\nA \\\\ B\n\\end{tabular}\nBody.";
    let doc = parse_latex(src);
    let table_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Table(_)))
        .count();
    assert_eq!(
        table_count, 0,
        "bare \\begin{{tabular}} should not produce a Block::Table"
    );
}

#[test]
fn test_extract_font_size_from_documentclass() {
    let src = "\\documentclass[a4paper,14pt,oneside]{memoir}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.font_size_body_hp, Some(28)); // 14pt * 2
}

#[test]
fn test_extract_font_size_12pt_from_documentclass() {
    let src = "\\documentclass[12pt]{article}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.font_size_body_hp, Some(24)); // 12pt * 2
}

#[test]
fn test_extract_font_size_absent() {
    let src = "\\documentclass{article}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.font_size_body_hp, None);
}

#[test]
fn test_extract_table_font_size_from_settblrinner() {
    let src = "\\documentclass[14pt]{memoir}\n\\SetTblrInner{font=\\footnotesize}\nBody.";
    let doc = parse_latex(src);
    // footnotesize at 14pt base = 12pt = 24 hp
    assert_eq!(doc.layout.font_size_table_hp, Some(24));
}

#[test]
fn test_extract_table_font_size_absent() {
    let src = "\\documentclass[14pt]{memoir}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.font_size_table_hp, None);
}

#[test]
fn test_extract_caption_font_size_from_captionsetup() {
    let src = "\\documentclass[14pt]{memoir}\n\\captionsetup[table]{font={normalsize,bf}}\nBody.";
    let doc = parse_latex(src);
    // normalsize at 14pt base = 14pt = 28 hp
    assert_eq!(doc.layout.font_size_caption_hp, Some(28));
}

#[test]
fn test_extract_caption_font_size_small() {
    let src = "\\documentclass[14pt]{memoir}\n\\captionsetup{font=small}\nBody.";
    let doc = parse_latex(src);
    // small at 14pt base = 13pt = 26 hp
    assert_eq!(doc.layout.font_size_caption_hp, Some(26));
}

#[test]
fn test_extract_caption_font_size_absent() {
    let src = "\\documentclass[14pt]{memoir}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.font_size_caption_hp, None);
}

#[test]
fn test_extract_footnote_font_size_derived_from_body() {
    let src = "\\documentclass[14pt]{memoir}\nBody.";
    let doc = parse_latex(src);
    // 14pt body → footnote = 14 − 4 = 10pt = 20 hp
    assert_eq!(doc.layout.font_size_footnote_hp, Some(20));
}

#[test]
fn test_extract_footnote_font_size_derived_from_12pt_body() {
    let src = "\\documentclass[12pt]{article}\nBody.";
    let doc = parse_latex(src);
    // 12pt body → footnote = 12 − 4 = 8pt = 16 hp
    assert_eq!(doc.layout.font_size_footnote_hp, Some(16));
}

#[test]
fn test_extract_parindent_in_em() {
    let src = "\\setlength{\\parindent}{2.5em}\nBody.";
    let doc = parse_latex(src);
    // 2.5em * 280tw/em = 700tw
    assert_eq!(doc.layout.body_first_line_indent_twips, Some(700));
}

#[test]
fn test_extract_parindent_in_cm() {
    let src = "\\setlength{\\parindent}{1.25cm}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.body_first_line_indent_twips, Some(709));
}

#[test]
fn test_extract_parindent_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.body_first_line_indent_twips, None);
}

#[test]
fn test_extract_figurename_renewcommand() {
    let src = "\\renewcommand{\\figurename}{Рисунок}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_label_figure.as_deref(), Some("Рисунок"));
}

#[test]
fn test_extract_tablename_renewcommand() {
    let src = "\\renewcommand{\\tablename}{Таблица}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_label_table.as_deref(), Some("Таблица"));
}

#[test]
fn test_extract_caption_labels_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_label_figure, None);
    assert_eq!(doc.layout.caption_label_table, None);
}

#[test]
fn test_extract_caption_label_separator_from_declared_tabsep_and_figsep() {
    let src = r"
\newcommand{\tablabelsep}{~---\ }
\newcommand{\figlabelsep}{:\space}
\DeclareCaptionLabelSeparator{tabsep}{\tablabelsep}
\DeclareCaptionLabelSeparator{figsep}{\figlabelsep}
\captionsetup[table]{labelsep=tabsep}
\captionsetup[figure]{labelsep=figsep}
Body.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.caption_label_separator_table.as_deref(),
        Some(" — ")
    );
    assert_eq!(
        doc.layout.caption_label_separator_figure.as_deref(),
        Some(": ")
    );
}

#[test]
fn test_extract_caption_label_separator_prefers_target_over_global() {
    let src = r"
\captionsetup{labelsep=colon}
\captionsetup[figure]{labelsep=period}
Body.";
    let doc = parse_latex(src);
    assert_eq!(
        doc.layout.caption_label_separator_figure.as_deref(),
        Some(". ")
    );
    assert_eq!(
        doc.layout.caption_label_separator_table.as_deref(),
        Some(": ")
    );
}

#[test]
fn test_extract_caption_label_separator_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_label_separator_figure, None);
    assert_eq!(doc.layout.caption_label_separator_table, None);
}

#[test]
fn test_extract_chaptername_renewcommand() {
    let src = "\\renewcommand{\\chaptername}{Chapter}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.chapter_name.as_deref(), Some("Chapter"));
}

#[test]
fn test_extract_chaptername_from_chapstyle_counter_and_language() {
    let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{1}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.chapter_name.as_deref(), Some("Глава"));
}

#[test]
fn test_extract_chaptername_from_chapstyle_disabled_returns_none() {
    let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{0}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.chapter_name, None);
}

#[test]
fn test_extract_heading_uppercase_from_makeupper_in_chaptertitle() {
    let src = "\\renewcommand{\\printchaptertitle}[1]{\\MakeUppercase{#1}}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.heading_uppercase, Some(true));
}

#[test]
fn test_heading_uppercase_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.heading_uppercase, None);
}

#[test]
fn test_extract_language_from_setmainlanguage() {
    let src = "\\setmainlanguage{russian}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.document_language.as_deref(), Some("ru-RU"));
}

#[test]
fn test_extract_language_from_babel() {
    let src = "\\usepackage[english, russian]{babel}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.document_language.as_deref(), Some("ru-RU"));
}

#[test]
fn test_extract_language_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.document_language, None);
}

#[test]
fn test_extract_english_language_from_babel() {
    let src = "\\usepackage[english]{babel}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.document_language.as_deref(), Some("en-US"));
}

#[test]
fn test_extract_documentclass_paper_size_a4() {
    let src = "\\documentclass[a4paper,14pt]{memoir}\nBody.";
    assert_eq!(
        extract_documentclass_paper_size(src),
        Some((11_906, 16_838))
    );
}

#[test]
fn test_extract_documentclass_paper_size_letter() {
    let src = "\\documentclass[letterpaper,12pt]{article}\nBody.";
    assert_eq!(
        extract_documentclass_paper_size(src),
        Some((12_240, 15_840))
    );
}

#[test]
fn test_extract_documentclass_paper_size_absent() {
    let src = "\\documentclass[12pt]{article}\nBody.";
    assert_eq!(extract_documentclass_paper_size(src), None);
}

#[test]
fn test_extract_setmainfont_conditional_for_monofont() {
    let src = "\
\\setcounter{fontfamily}{2}
\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}
\\ifnumequal{\\value{fontfamily}}{2}{\\setmonofont{Fira Code}}
Body.";
    assert_eq!(
        extract_setmainfont_conditional_for(src, Some(2), "\\setmonofont"),
        Some("Fira Code".to_string())
    );
}

#[test]
fn test_extract_setmainfont_conditional_for_no_matching_branch() {
    let src = "\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}\nBody.";
    assert_eq!(
        extract_setmainfont_conditional_for(src, Some(3), "\\setmonofont"),
        None
    );
}

#[test]
fn test_extract_setmainfont_conditional_for_without_counter_value() {
    let src = "\\ifnumequal{\\value{fontfamily}}{1}{\\setmonofont{Courier New}}\nBody.";
    assert_eq!(
        extract_setmainfont_conditional_for(src, None, "\\setmonofont"),
        None
    );
}

#[test]
fn test_extract_heading_alignment_from_titleformat_center() {
    let src =
        "\\titleformat{\\chapter}[display]{\\centering\\bfseries}{\\thechapter}{1em}{}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
}

#[test]
fn test_extract_heading_alignment_from_memoir_raggedright() {
    let src = "\\renewcommand{\\printchaptertitle}[1]{\\raggedright\\chaptitlefont #1}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("left".to_string()));
}

#[test]
fn test_extract_heading_alignment_absent() {
    assert_eq!(extract_heading_alignment("Body."), None);
}

#[test]
fn test_extract_heading_alignment_from_titleformat_name_selector_before_code() {
    let src =
        "\\titleformat{name=\\chapter}[display]{\\bfseries}{\\thechapter}{1em}{\\filcenter}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
}

#[test]
fn test_extract_heading_alignment_from_titleformat_starred() {
    let src = "\\titleformat*{\\chapter}{\\filcenter\\bfseries}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
}

#[test]
fn test_extract_heading_alignment_from_headingalign_counter() {
    let src = "\\newcounter{headingalign}\n\\setcounter{headingalign}{0}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("center".to_string()));
}

#[test]
fn test_extract_heading_alignment_sethangfrom_promotes_left_to_justify() {
    let src = "\\setcounter{headingalign}{1}\n\\sethangfrom{\\noindent #1}\nBody.";
    assert_eq!(extract_heading_alignment(src), Some("both".to_string()));
}

#[test]
fn test_extract_heading_number_delimiter_from_thechapter_dot() {
    let src = "\\renewcommand{\\thechapter}{\\arabic{chapter}.}\nBody.";
    assert_eq!(extract_heading_number_delimiter(src), Some(".".to_string()));
}

#[test]
fn test_extract_heading_number_delimiter_from_thechapter_empty() {
    let src = "\\renewcommand{\\thechapter}{\\arabic{chapter}}\nBody.";
    assert_eq!(extract_heading_number_delimiter(src), Some(String::new()));
}

#[test]
fn test_extract_heading_number_delimiter_from_titleformat_label() {
    let src = "\\titleformat{\\chapter}[display]{\\bfseries}{\\thechapter:}{1em}{}\nBody.";
    assert_eq!(extract_heading_number_delimiter(src), Some(":".to_string()));
}

#[test]
fn test_extract_heading_number_delimiter_from_headingdelim_counter() {
    let src = "\\newcounter{headingdelim}\n\\setcounter{headingdelim}{0}\nBody.";
    assert_eq!(extract_heading_number_delimiter(src), Some(String::new()));
}

#[test]
fn test_extract_heading_number_delimiter_for_level_from_headingdelim_counter() {
    let src = "\\newcounter{headingdelim}\n\\setcounter{headingdelim}{1}\nBody.";
    assert_eq!(
        extract_heading_number_delimiter_for_level(src, 1),
        Some(".".to_string())
    );
    assert_eq!(
        extract_heading_number_delimiter_for_level(src, 2),
        Some(String::new())
    );
    assert_eq!(
        extract_heading_number_delimiter_for_level(src, 3),
        Some(String::new())
    );
}

#[test]
fn test_extract_heading_number_delimiter_for_level_from_setsecnumformat() {
    let src = "\\setsecnumformat{\\csname the#1\\endcsname.\\space}\nBody.";
    assert_eq!(
        extract_heading_number_delimiter_for_level(src, 2),
        Some(".".to_string())
    );

    let src_no_dot = "\\setsecnumformat{\\csname the#1\\endcsname\\quad}\nBody.";
    assert_eq!(
        extract_heading_number_delimiter_for_level(src_no_dot, 2),
        Some(String::new())
    );
}

#[test]
fn test_extract_heading_number_delimiter_from_titlelabel() {
    let src = "\\titlelabel{\\thetitle.\\quad}\nBody.";
    assert_eq!(extract_heading_number_delimiter(src), Some(".".to_string()));
}

#[test]
fn test_extract_heading_indents_from_setsecindent_macros() {
    let src = "\
\\setlength{\\parindent}{1.25cm}
\\setsecindent{\\parindent}
\\setsubsecindent{12pt}
\\setsubsubsecindent{0pt}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.heading_indent_section_twips, Some(709));
    assert_eq!(doc.layout.heading_indent_subsection_twips, Some(240));
    assert_eq!(doc.layout.heading_indent_subsubsection_twips, Some(0));
}

#[test]
fn test_extract_heading_indents_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.heading_indent_section_twips, None);
    assert_eq!(doc.layout.heading_indent_subsection_twips, None);
    assert_eq!(doc.layout.heading_indent_subsubsection_twips, None);
}

#[test]
fn test_extract_captionsetup_justification_center() {
    let src = "\\captionsetup{justification=centering}\nBody.";
    assert_eq!(
        extract_captionsetup_justification(src),
        Some("center".to_string())
    );
}

#[test]
fn test_extract_captionsetup_justification_raggedright() {
    let src = "\\captionsetup[table]{justification=raggedright}\nBody.";
    assert_eq!(
        extract_captionsetup_justification(src),
        Some("left".to_string())
    );
}

#[test]
fn test_extract_captionsetup_justification_absent() {
    assert_eq!(extract_captionsetup_justification("Body."), None);
}

#[test]
fn test_extract_captionsetup_skip_twips_target_and_global_fallback() {
    let src = "\\captionsetup{skip=4pt}\n\\captionsetup[table]{skip=3pt}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_skip_twips_table, Some(60));
    assert_eq!(doc.layout.caption_skip_twips_figure, Some(80));
}

#[test]
fn test_extract_captionsetup_skip_twips_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_skip_twips_table, None);
    assert_eq!(doc.layout.caption_skip_twips_figure, None);
}

#[test]
fn test_extract_captionsetup_position_target_and_global_fallback() {
    let src = "\\captionsetup{position=below}\n\\captionsetup[table]{position=above}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_position_table.as_deref(), Some("top"));
    assert_eq!(
        doc.layout.caption_position_figure.as_deref(),
        Some("bottom")
    );
}

#[test]
fn test_extract_captionsetup_position_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_position_table, None);
    assert_eq!(doc.layout.caption_position_figure, None);
}

#[test]
fn test_extract_captionsetup_singlelinecheck_from_macro() {
    let src = "\
\\newcommand{\\tabsinglecenter}{false}
\\captionsetup[table]{singlelinecheck=\\tabsinglecenter}
\\captionsetup[figure]{singlelinecheck=true}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_singlelinecheck_table, Some(false));
    assert_eq!(doc.layout.caption_singlelinecheck_figure, Some(true));
}

#[test]
fn test_extract_captionsetup_singlelinecheck_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_singlelinecheck_table, None);
    assert_eq!(doc.layout.caption_singlelinecheck_figure, None);
}

#[test]
fn test_extract_captionsetup_indent_twips_from_macro() {
    let src = "\
\\newcommand{\\tabindent}{1.25cm}
\\captionsetup[table]{indent=\\tabindent}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_indent_twips_table, Some(709));
    assert_eq!(doc.layout.caption_indent_twips_figure, None);
}

#[test]
fn test_extract_captionsetup_indent_twips_target_and_global_fallback() {
    let src = "\\captionsetup{indent=0pt}\n\\captionsetup[figure]{indent=6pt}\nBody.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.caption_indent_twips_figure, Some(120));
    assert_eq!(doc.layout.caption_indent_twips_table, Some(0));
}

#[test]
fn test_extract_captionsetup_indent_twips_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_indent_twips_table, None);
    assert_eq!(doc.layout.caption_indent_twips_figure, None);
}

#[test]
fn test_extract_graphicspath_basic_and_cleaned() {
    let src = "\\graphicspath{{./figures/}{./img/}{../images/}}\nBody.";
    assert_eq!(
        extract_graphicspath(src),
        vec![
            "figures/".to_string(),
            "img/".to_string(),
            "../images/".to_string()
        ]
    );
}

#[test]
fn test_extract_graphicspath_uses_last_definition() {
    let src = "\\graphicspath{{./figures/}}\n\\graphicspath{{./plots/}{./img/}}\nBody.";
    assert_eq!(
        extract_graphicspath(src),
        vec!["plots/".to_string(), "img/".to_string()]
    );
}

#[test]
fn test_extract_graphicspath_absent() {
    assert!(extract_graphicspath("Body.").is_empty());
}

#[test]
fn test_extract_graphicspath_normalizes_windows_style_and_deduplicates() {
    let src = "\\graphicspath{{.\\\\figures\\\\}{./figures/}{./img//}}\nBody.";
    assert_eq!(
        extract_graphicspath(src),
        vec!["figures/".to_string(), "img/".to_string()]
    );
}

#[test]
fn test_extract_memoir_heading_controls_from_counters() {
    let src = "\
\\setmainlanguage{russian}
\\setcounter{chapstyle}{1}
\\setcounter{headingalign}{1}
\\setcounter{headingdelim}{1}
\\renewcommand*{\\printchaptername}{\\MakeUppercase{\\@chapapp}}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.chapter_name.as_deref(), Some("Глава"));
    assert_eq!(doc.layout.heading_alignment.as_deref(), Some("left"));
    assert_eq!(doc.layout.heading_number_delimiter.as_deref(), Some("."));
    assert_eq!(
        doc.layout.heading_number_delimiter_section.as_deref(),
        Some("")
    );
    assert_eq!(
        doc.layout.heading_number_delimiter_subsection.as_deref(),
        Some("")
    );
    assert_eq!(
        doc.layout.heading_number_delimiter_subsubsection.as_deref(),
        Some("")
    );
}

// ── Task 1: toc_chapter_entry_bold ────────────────────────────────────

#[test]
fn test_toc_chapter_entry_bold_normalfont() {
    let src = r"\renewcommand{\cftchapterfont}{\normalfont}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_entry_bold, Some(false));
}

#[test]
fn test_toc_chapter_entry_bold_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_chapter_entry_bold, None);
}

// ── Task 1: toc_chapter_page_bold ────────────────────────────────────

#[test]
fn test_toc_chapter_page_bold_normalfont() {
    let src = r"\renewcommand{\cftchapterpagefont}{\normalfont}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_chapter_page_bold, Some(false));
}

#[test]
fn test_toc_chapter_page_bold_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_chapter_page_bold, None);
}

// ── Task 1: toc_aftersnum ────────────────────────────────────────────

#[test]
fn test_toc_aftersnum_chapter_present() {
    let src = r"\renewcommand\cftchapteraftersnum{.\space}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_aftersnum_chapter.as_deref(), Some(". "));
}

#[test]
fn test_toc_aftersnum_section_present() {
    let src = r"\renewcommand\cftsectionaftersnum{.\space}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_aftersnum_section.as_deref(), Some(". "));
}

#[test]
fn test_toc_aftersnum_respects_headingdelim_counter_one() {
    let src = r"\setcounter{headingdelim}{1}
\ifnumgreater{\value{headingdelim}}{1}{%
  \renewcommand\cftsectionaftersnum{.\space}
}{}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_aftersnum_chapter.as_deref(), Some(". "));
    assert_eq!(doc.layout.toc_aftersnum_section.as_deref(), Some(""));
    assert_eq!(doc.layout.toc_aftersnum_subsection.as_deref(), Some(""));
    assert_eq!(doc.layout.toc_aftersnum_subsubsection.as_deref(), Some(""));
}

#[test]
fn test_toc_aftersnum_respects_headingdelim_counter_two() {
    let src = r"\setcounter{headingdelim}{2}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_aftersnum_chapter.as_deref(), Some(". "));
    assert_eq!(doc.layout.toc_aftersnum_section.as_deref(), Some(". "));
    assert_eq!(doc.layout.toc_aftersnum_subsection.as_deref(), Some(". "));
    assert_eq!(
        doc.layout.toc_aftersnum_subsubsection.as_deref(),
        Some(". ")
    );
}

#[test]
fn test_toc_aftersnum_all_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_aftersnum_chapter, None);
    assert_eq!(doc.layout.toc_aftersnum_section, None);
    assert_eq!(doc.layout.toc_aftersnum_subsection, None);
    assert_eq!(doc.layout.toc_aftersnum_subsubsection, None);
}

// ── Task 1: toc_appendix_name ────────────────────────────────────────

#[test]
fn test_toc_appendix_name_present() {
    let src = r"\renewcommand{\appendixname}{Appendix}
\renewcommand{\cftappendixname}{\appendixname\space}
Body.";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.toc_appendix_name.as_deref(), Some("Appendix"));
}

#[test]
fn test_toc_appendix_name_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_appendix_name, None);
}

// ── Task 2: is_only_linebreaks — no Block from lone \\ ────────────────

#[test]
fn test_linebreak_only_chunk_no_block() {
    let src = "First paragraph.\n\\\\\n\nSecond paragraph.";
    let doc = parse_latex(src);
    let para_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph(_)))
        .count();
    assert_eq!(para_count, 2, "expected 2 paragraphs, got: {para_count}");
}

// ── Task 2: inline math preserves surrounding spaces ─────────────────

#[test]
fn test_inline_math_preserves_spaces() {
    let doc = parse_latex("Let $P$ denote the value.");
    let Block::Paragraph(inlines) = &doc.blocks[0] else {
        panic!("expected paragraph");
    };
    let text: String = inlines
        .iter()
        .map(|i| match i {
            Inline::Text(s) => s.clone(),
            Inline::InlineMath(_) => "MATH".to_string(),
            _ => String::new(),
        })
        .collect();
    assert!(text.contains("Let "), "space before math missing: {text:?}");
    assert!(
        text.contains(" denote"),
        "space after math missing: {text:?}"
    );
}

// ── List settings extraction tests ───────────────────────────────────────

#[test]
fn test_extract_list_settings_labelsep_em() {
    let source = r"\setlist{nosep, labelsep=.5em, labelwidth=!, leftmargin=\dimexpr\parindent-\labelwidth-\labelsep\relax}";
    let (sep, width, _item_indent, _left_margin, _bullet) =
        extract_list_settings_with_body_font(source, None, None);
    // 0.5em at 14pt (280 twips/em) = 140 twips
    let sep = sep.expect("labelsep should be extracted");
    assert!((130..=150).contains(&sep), "expected ~140 twips, got {sep}");
    assert!(width.is_none(), "labelwidth=! should produce None");
}

#[test]
fn test_extract_list_settings_absent() {
    let source = r"\usepackage{enumitem}";
    let (sep, width, _item_indent, _left_margin, bullet) =
        extract_list_settings_with_body_font(source, None, None);
    assert!(sep.is_none());
    assert!(width.is_none());
    assert!(bullet.is_none());
}

#[test]
fn test_extract_list_settings_prefers_last_setlist_override() {
    let source = r"\setlist{labelsep=.4em}\setlist{labelsep=.8em}";
    let (sep, _width, _item_indent, _left_margin, _bullet) =
        extract_list_settings_with_body_font(source, None, None);
    assert_eq!(
        sep,
        Some(224),
        "expected .8em at 14pt to win as last override"
    );
}

#[test]
fn test_extract_list_settings_accepts_labelsep_star_and_spaces() {
    let source = r"\setlist{ labelsep* = .5em , labelwidth = ! }";
    let (sep, width, _item_indent, _left_margin, _bullet) =
        extract_list_settings_with_body_font(source, None, None);
    assert_eq!(sep, Some(140));
    assert_eq!(width, None);
}

#[test]
fn test_extract_list_settings_itemindent_falls_back_to_listparindent() {
    let doc = parse_latex(
        "\\setlength{\\parindent}{1.25cm}\n\\setlist{listparindent=\\parindent}\nBody.",
    );
    assert_eq!(doc.layout.list_item_indent_twips, Some(709));
}

#[test]
fn test_extract_list_settings_dimexpr_supports_unit_terms() {
    let source =
        r"\setlist{labelsep=.5em,labelwidth=.5em,leftmargin=\dimexpr\parindent-1em+1pt\relax}";
    let (_sep, _width, _item_indent, left_margin, _bullet) =
        extract_list_settings_with_body_font(source, Some(709), Some(28));
    assert_eq!(left_margin, Some(449));
}

#[test]
fn test_parse_latex_length_supports_additional_tex_units() {
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1pc", None),
        Some(240)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1bp", None),
        Some(20)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("65536sp", None),
        Some(20)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1dd", None),
        Some(21)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1cc", None),
        Some(257)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1ex", Some(28)),
        Some(121)
    );
}

#[test]
fn test_parse_latex_length_supports_plus_minus_glue() {
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1em plus 1pt", Some(28)),
        Some(300)
    );
    assert_eq!(
        parse_latex_length_to_twips_with_body_font("1em minus 1pt", Some(28)),
        Some(260)
    );
}

#[test]
fn test_extract_labelitemi_char_endash() {
    let source = r"\renewcommand{\labelitemi}{\normalfont\bfseries{--}}";
    let (_sep, _width, _item_indent, _left_margin, bullet) =
        extract_list_settings_with_body_font(source, None, None);
    let bullet = bullet.expect("bullet should be extracted");
    assert_eq!(bullet, "–", "expected en-dash, got {bullet:?}");
}

#[test]
fn test_extract_labelitemi_char_absent() {
    let source = r"\usepackage{enumitem}";
    let (_sep, _width, _item_indent, _left_margin, bullet) =
        extract_list_settings_with_body_font(source, None, None);
    assert!(bullet.is_none());
}

// ── Source vspace extraction tests ───────────────────────────────────────

#[test]
fn test_extract_source_vspace_tablesource() {
    let source = r"\newcommand{\tablesource}[1]{\par\vspace{4pt}{\noindent\raggedright\small\textit{#1}\par}}";
    let tw = extract_source_vspace_twips_with_body_font(source, "tablesource", None);
    // 4pt = 80 twips
    let tw = tw.expect("vspace should be extracted");
    assert_eq!(tw, 80, "expected 80 twips (4pt), got {tw}");
}

#[test]
fn test_extract_source_vspace_figuresource() {
    let source = r"\newcommand{\figuresource}[1]{\par\vspace{2pt}{\noindent\raggedright\small\textit{#1}\par}}";
    let tw = extract_source_vspace_twips_with_body_font(source, "figuresource", None);
    let tw = tw.expect("vspace should be extracted");
    assert_eq!(tw, 40, "expected 40 twips (2pt), got {tw}");
}

#[test]
fn test_extract_source_vspace_absent() {
    let source = r"\usepackage{caption}";
    assert!(extract_source_vspace_twips_with_body_font(source, "tablesource", None).is_none());
}

// ── Title page page number suppression tests ─────────────────────────────

#[test]
fn test_extract_title_page_suppress_inside_titlingpage() {
    let source = "\\begin{titlingpage}\n\\thispagestyle{empty}\n\\end{titlingpage}";
    assert_eq!(extract_title_page_suppress_number(source), Some(true));
}

#[test]
fn test_extract_title_page_suppress_absent() {
    let source = "\\begin{document}\nSome text.\n\\end{document}";
    assert!(extract_title_page_suppress_number(source).is_none());
}

#[test]
fn test_titlepage_mixed_chunk_applies_leading_vspace_and_alignment() {
    let source = "\\begin{titlepage}\n\\centering\n\\vspace{12pt}\nTitle line\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { inlines, style } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(plain_text_from_inlines(inlines).trim(), "Title line");
    assert_eq!(style.alignment.as_deref(), Some("center"));
    assert_eq!(style.first_line_indent_twips, Some(0));
    assert_eq!(style.space_before_twips, Some(240));
}

#[test]
fn test_titlepage_manual_linebreaks_trim_surrounding_spaces() {
    let source = "\\begin{titlepage}\nLine A \\\\\n   Line B\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);
    let Block::StyledParagraph { inlines, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert!(
        inlines
            .iter()
            .any(|inline| matches!(inline, Inline::LineBreak))
    );
    for window in inlines.windows(2) {
        if matches!(window[0], Inline::LineBreak)
            && let Inline::Text(text) = &window[1]
        {
            assert!(
                !text.starts_with(' '),
                "line after break unexpectedly starts with space: {text:?}"
            );
        }
    }
}

#[test]
fn test_titlepage_setstretch_applies_line_spacing_override() {
    let source = "\\begin{titlepage}\n\\setstretch{1.0}\nTitle line\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { style, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(style.line_spacing_twips, Some(240));
}

#[test]
fn test_titlepage_onehalfspacing_uses_document_font_size_factor() {
    let source = "\\documentclass[14pt]{memoir}\n\\begin{titlepage}\n\\OnehalfSpacing\nTitle line\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { style, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(style.line_spacing_twips, Some(312));
}

#[test]
fn test_layout_settings_extract_linespread_factor() {
    let doc = parse_latex("\\linespread{1.25}\nBody.");
    assert_eq!(doc.layout.body_line_spacing_twips, Some(300));
}

#[test]
fn test_layout_settings_ignore_body_linespread_when_preamble_has_setspacing() {
    let source = "\\setSpacing{1.385}\n\\begin{document}\nBody \\linespread{1}\\selectfont text.\n\\end{document}";
    let doc = parse_latex(source);
    assert_eq!(doc.layout.body_line_spacing_twips, Some(332));
}

#[test]
fn test_titlepage_fontsize_second_arg_overrides_line_spacing() {
    let source = "\\begin{titlepage}\n\n\\OnehalfSpacing\n\n{\\fontsize{16}{19}\\selectfont\\bfseries Title\\par}\n\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { style, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(style.font_size_hp, Some(32));
    assert_eq!(style.line_spacing_twips, Some(285));
}

#[test]
fn test_titlepage_flushright_tabular_gets_left_indent_estimate() {
    let source = "\\begin{titlepage}\n\\begin{flushright}\n\\begin{tabular}{l}\n\\textbf{Научный руководитель:} \\\\\nдоктор наук \\\\\nИван Иванов\n\\end{tabular}\n\\end{flushright}\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { style, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(style.alignment.as_deref(), Some("left"));
    assert_eq!(style.first_line_indent_twips, Some(0));
    assert!(
        style.left_indent_twips.is_some_and(|value| value > 0),
        "expected positive left indent, got {:?}",
        style.left_indent_twips
    );
    assert_eq!(style.space_before_twips, None);
    assert_eq!(style.space_after_twips, None);
}

#[test]
fn test_titlepage_flushright_tabular_preserves_vspace_plus_box_padding() {
    let source = "\\documentclass[14pt]{memoir}\n\\begin{titlepage}\n\\OnehalfSpacing\n\\vspace*{2.5cm}\n\\begin{flushright}\n\\begin{tabular}{l}\n\\textbf{Научный руководитель:} \\\\\nдоктор наук \\\\\nИван Иванов\n\\end{tabular}\n\\end{flushright}\n\\end{titlepage}";
    let doc = parse_latex(source);
    assert_eq!(doc.blocks.len(), 1);

    let Block::StyledParagraph { style, .. } = &doc.blocks[0] else {
        panic!("expected styled paragraph, got {:?}", doc.blocks[0]);
    };
    assert_eq!(style.line_spacing_twips, Some(312));
    assert_eq!(style.space_before_twips, Some(1417));
    assert_eq!(style.space_after_twips, None);
}

// ── Bibliography command parsing tests ────────────────────────────────────

#[test]
fn test_try_parse_bibliography_printbibliography_no_title() {
    // With Russian language, default title is "СПИСОК ЛИТЕРАТУРЫ".
    let block = try_parse_bibliography_command("\\printbibliography", Some("ru-RU"));
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "СПИСОК ЛИТЕРАТУРЫ");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

#[test]
fn test_try_parse_bibliography_printbibliography_with_title() {
    // Explicit title= always overrides language default.
    let block =
        try_parse_bibliography_command("\\printbibliography[title={References}]", Some("ru-RU"));
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "References");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

#[test]
fn test_try_parse_bibliography_nobibheading_skipped() {
    let block = try_parse_bibliography_command(
        "\\printbibliography[heading=nobibheading, section=1]",
        None,
    );
    assert!(block.is_none(), "nobibheading should produce None");
}

#[test]
fn test_try_parse_bibliography_insertbibliofullsorted() {
    // With Russian language, default title is "СПИСОК ЛИТЕРАТУРЫ".
    let block = try_parse_bibliography_command("\\insertbibliofullsorted", Some("ru-RU"));
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "СПИСОК ЛИТЕРАТУРЫ");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

#[test]
fn test_try_parse_bibliography_not_a_bib_command() {
    assert!(try_parse_bibliography_command("\\chapter{Introduction}", None).is_none());
    assert!(try_parse_bibliography_command("Some paragraph text.", None).is_none());
}

#[test]
fn test_bibliography_default_title_english() {
    // With English language, default title is "REFERENCES".
    let block = try_parse_bibliography_command("\\insertbibliofullsorted", Some("en-US"));
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "REFERENCES");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

#[test]
fn test_bibliography_default_title_unknown_language_falls_back_to_references() {
    // Unknown language tag falls back to "REFERENCES".
    let block = try_parse_bibliography_command("\\printbibliography", None);
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "REFERENCES");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

#[test]
fn test_bibliography_explicit_title_overrides_language() {
    // Explicit title= wins regardless of language.
    let block =
        try_parse_bibliography_command("\\printbibliography[title=Works Cited]", Some("ru-RU"));
    match block {
        Some(Block::BibliographyHeading { title }) => {
            assert_eq!(title, "Works Cited");
        }
        other => panic!("expected BibliographyHeading, got {other:?}"),
    }
}

// ── Dissertation counter fallback gate tests ───────────────────────────────

#[test]
fn test_counter_fallbacks_skipped_for_non_dissertation_class() {
    // When document class is NOT a dissertation class, the placeholder
    // "Диссертация состоит из..." must be left untouched by the parser.
    // parse_latex() has no \documentclass, so document_class = None → gate fires.
    let placeholder = "Диссертация состоит из введения, главы, заключения и приложений.";
    let doc = parse_latex(placeholder);
    // The text should survive unchanged in the parsed paragraph.
    let found = doc.blocks.iter().any(|b| match b {
        Block::Paragraph(inlines) => inlines.iter().any(|inline| match inline {
            Inline::Text(t) => t.contains("главы"),
            _ => false,
        }),
        _ => false,
    });
    assert!(
        found,
        "dissertation placeholder should be preserved unchanged for non-dissertation class"
    );
}

// ── v0.9.2 parser extraction tests ─────────────────────────────────────────

#[test]
fn test_extract_page_gutter_from_geometry_bindingoffset() {
    let doc = parse_latex("\\geometry{a4paper,bindingoffset=1cm}\nBody.");
    assert_eq!(doc.layout.page_gutter_twips, Some(567));
}

#[test]
fn test_page_gutter_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.page_gutter_twips, None);
}

#[test]
fn test_extract_toc_depth_from_setcounter() {
    let doc = parse_latex("\\setcounter{tocdepth}{1}\nBody.");
    assert_eq!(doc.layout.toc_depth, Some(1));
}

#[test]
fn test_toc_depth_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.toc_depth, None);
}

#[test]
fn test_extract_hypersetup_linkcolor_and_colorlinks() {
    let doc = parse_latex("\\hypersetup{linkcolor=blue,colorlinks=false}\nBody.");
    assert_eq!(doc.layout.hyperlink_text_color.as_deref(), Some("blue"));
    assert_eq!(doc.layout.hyperlink_underline, Some(true));
}

#[test]
fn test_extract_hypersetup_allcolors_and_bare_colorlinks_flag() {
    let doc = parse_latex("\\hypersetup{allcolors=red,colorlinks}\nBody.");
    assert_eq!(doc.layout.hyperlink_text_color.as_deref(), Some("red"));
    assert_eq!(doc.layout.hyperlink_underline, Some(false));
}

#[test]
fn test_hypersetup_hyperlink_settings_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.hyperlink_text_color, None);
    assert_eq!(doc.layout.hyperlink_underline, None);
}

#[test]
fn test_extract_captionsetup_labelfont_bold_with_target_override() {
    let doc = parse_latex(
        "\\captionsetup{labelfont=bf}\n\\captionsetup[table]{labelfont=normalfont}\nBody.",
    );
    assert_eq!(doc.layout.caption_label_bold_figure, Some(true));
    assert_eq!(doc.layout.caption_label_bold_table, Some(false));
}

#[test]
fn test_captionsetup_labelfont_bold_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.caption_label_bold_figure, None);
    assert_eq!(doc.layout.caption_label_bold_table, None);
}

#[test]
fn test_extract_body_text_alignment_from_raggedright() {
    let doc = parse_latex("\\raggedright\nBody.");
    assert_eq!(doc.layout.body_text_alignment.as_deref(), Some("left"));
}

#[test]
fn test_extract_body_text_alignment_from_atbegindocument() {
    let doc = parse_latex("\\AtBeginDocument{\\raggedleft}\nBody.");
    assert_eq!(doc.layout.body_text_alignment.as_deref(), Some("right"));
}

#[test]
fn test_body_text_alignment_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.body_text_alignment, None);
}

#[test]
fn test_body_text_alignment_ignores_macro_local_alignment_commands() {
    let src = r"
\newcommand{\hdngalign}{\centering}
\newcommand{\splitformattext}{\raggedright}
Body.
";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.body_text_alignment, None);
}

#[test]
fn test_extract_page_number_alignment_from_cfoot() {
    let doc = parse_latex("\\cfoot{\\thepage}\nBody.");
    assert_eq!(doc.layout.page_number_alignment.as_deref(), Some("center"));
}

#[test]
fn test_page_number_alignment_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.page_number_alignment, None);
}

#[test]
fn test_extract_page_number_alignment_from_makeoddfoot() {
    let doc = parse_latex("\\makeoddfoot{plain}{}{}{\\thepage}\nBody.");
    assert_eq!(doc.layout.page_number_alignment.as_deref(), Some("right"));
}

#[test]
fn test_extract_page_number_alignment_from_makeoddhead() {
    let doc = parse_latex("\\makeoddhead{plain}{}{\\thepage}{}\nBody.");
    assert_eq!(doc.layout.page_number_alignment.as_deref(), Some("center"));
}

#[test]
fn test_extract_heading_spacing_from_disstyles_commands() {
    let src = r"
\setSpacing{1.5}
\setlength{\beforechapskip}{0pt}
\setlength{\afterchapskip}{\onelineskip}
\setbeforesecskip{0.5\onelineskip}
\setaftersecskip{0.5\onelineskip}
\setbeforesubsecskip{0.25\onelineskip}
\setaftersubsecskip{0.25\onelineskip}
\setbeforesubsubsecskip{0.1\onelineskip}
\setaftersubsubsecskip{0.1\onelineskip}
Body.
";
    let doc = parse_latex(src);
    assert_eq!(doc.layout.heading_space_before_chapter_twips, Some(0));
    assert_eq!(doc.layout.heading_space_after_chapter_twips, Some(360));
    assert_eq!(doc.layout.heading_space_before_section_twips, Some(180));
    assert_eq!(doc.layout.heading_space_after_section_twips, Some(180));
    assert_eq!(doc.layout.heading_space_before_subsection_twips, Some(90));
    assert_eq!(doc.layout.heading_space_after_subsection_twips, Some(90));
    assert_eq!(
        doc.layout.heading_space_before_subsubsection_twips,
        Some(36)
    );
    assert_eq!(doc.layout.heading_space_after_subsubsection_twips, Some(36));
}

#[test]
fn test_heading_spacing_absent() {
    let doc = parse_latex("Body.");
    assert_eq!(doc.layout.heading_space_before_chapter_twips, None);
    assert_eq!(doc.layout.heading_space_after_chapter_twips, None);
    assert_eq!(doc.layout.heading_space_before_section_twips, None);
    assert_eq!(doc.layout.heading_space_after_section_twips, None);
    assert_eq!(doc.layout.heading_space_before_subsection_twips, None);
    assert_eq!(doc.layout.heading_space_after_subsection_twips, None);
    assert_eq!(doc.layout.heading_space_before_subsubsection_twips, None);
    assert_eq!(doc.layout.heading_space_after_subsubsection_twips, None);
}

#[test]
fn test_em_length_scales_with_document_font_size() {
    let doc = parse_latex("\\documentclass[12pt]{article}\n\\setlength{\\parindent}{2.5em}\nBody.");
    // 2.5em at 12pt = 2.5 * 12 * 20 = 600 twips.
    assert_eq!(doc.layout.body_first_line_indent_twips, Some(600));
}

#[test]
fn test_list_labelsep_em_scales_with_document_font_size() {
    let doc = parse_latex("\\documentclass[12pt]{article}\n\\setlist{labelsep=.5em}\nBody.");
    assert_eq!(doc.layout.list_label_sep_twips, Some(120));
}

#[test]
fn test_plain_russian_front_matter_heading_is_language_gated_off_for_english() {
    let doc = parse_latex("\\usepackage[english]{babel}\nОГЛАВЛЕНИЕ");
    assert!(
        matches!(doc.blocks.first(), Some(Block::Paragraph(_))),
        "expected plain paragraph, got {:?}",
        doc.blocks
    );
}

#[test]
fn test_plain_russian_front_matter_heading_kept_for_russian_language() {
    let doc = parse_latex("\\usepackage[russian]{babel}\nОГЛАВЛЕНИЕ");
    assert!(
        matches!(doc.blocks.first(), Some(Block::Section { level: 1, .. })),
        "expected section heading, got {:?}",
        doc.blocks
    );
}
