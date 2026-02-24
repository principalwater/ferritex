    use super::*;
    use crate::model::{Block, Document, Inline, Table, TableCell, TableRow, TocEntry};
    use docx_rs::BuildXML;

    #[test]
    fn float_number_is_chapter_aware() {
        assert_eq!(float_number(0, 3), "3");
        assert_eq!(float_number(2, 4), "2.4");
    }

    #[test]
    fn caption_prefix_detection_is_case_insensitive() {
        let prefixed = vec![Inline::Text("Таблица 1. Пример".to_string())];
        let plain = vec![Inline::Text("Пример".to_string())];
        assert!(caption_is_prefixed("ТАБЛИЦА", &prefixed));
        assert!(!caption_is_prefixed("Таблица", &plain));
    }

    #[test]
    fn caption_prefix_uses_configured_separator() {
        assert_eq!(
            caption_prefix_text("Table", Some("2"), " --- "),
            Some("Table 2 --- ".to_string())
        );
        assert_eq!(
            caption_prefix_text("Figure", Some("3"), ": "),
            Some("Figure 3: ".to_string())
        );
    }

    #[test]
    fn caption_paragraph_top_position_sets_after_skip_and_indent() {
        let inlines = vec![Inline::Text("Sample caption".to_string())];
        let para = caption_paragraph(
            "Table",
            Some("1"),
            ". ",
            &inlines,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 120,
                skip_twips: 60,
                position: CaptionPosition::Top,
                singlelinecheck: false,
                footnote_font_size_hp: 20,
            },
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");

        assert!(xml.contains("w:after=\"60\""), "xml: {xml}");
        assert!(xml.contains("w:left=\"120\""), "xml: {xml}");
        assert!(xml.contains("w:jc w:val=\"left\""), "xml: {xml}");
    }

    #[test]
    fn caption_paragraph_bottom_position_sets_before_skip() {
        let inlines = vec![Inline::Text("Sample caption".to_string())];
        let para = caption_paragraph(
            "Figure",
            Some("2"),
            ". ",
            &inlines,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Center,
                indent_twips: 0,
                skip_twips: 40,
                position: CaptionPosition::Bottom,
                singlelinecheck: false,
                footnote_font_size_hp: 20,
            },
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");

        assert!(xml.contains("w:before=\"40\""), "xml: {xml}");
    }

    #[test]
    fn caption_singlelinecheck_centers_short_caption_only() {
        let short = vec![Inline::Text("Short caption".to_string())];
        let centered = caption_paragraph(
            "Figure",
            Some("1"),
            ". ",
            &short,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 0,
                skip_twips: 0,
                position: CaptionPosition::Bottom,
                singlelinecheck: true,
                footnote_font_size_hp: 20,
            },
        );
        let centered_xml =
            String::from_utf8(centered.build()).expect("paragraph xml should be utf8");
        assert!(
            centered_xml.contains("w:jc w:val=\"center\""),
            "xml: {centered_xml}"
        );

        let long_text = "very long caption text ".repeat(8);
        let long = vec![Inline::Text(long_text)];
        let kept = caption_paragraph(
            "Figure",
            Some("1"),
            ". ",
            &long,
            CaptionRenderSettings {
                default_alignment: AlignmentType::Left,
                indent_twips: 0,
                skip_twips: 0,
                position: CaptionPosition::Bottom,
                singlelinecheck: true,
                footnote_font_size_hp: 20,
            },
        );
        let kept_xml = String::from_utf8(kept.build()).expect("paragraph xml should be utf8");
        assert!(kept_xml.contains("w:jc w:val=\"left\""), "xml: {kept_xml}");
    }

    #[test]
    fn chapter_prefix_does_not_duplicate_trailing_dot() {
        let profile = RenderProfile::from_layout(&DocumentLayout {
            chapter_name: Some("Глава".to_string()),
            heading_number_delimiter: Some(".".to_string()),
            ..DocumentLayout::default()
        });
        let para = build_paragraph(
            &Block::Section {
                level: 1,
                number: Some("1.".to_string()),
                label: None,
                title: vec![Inline::Text("Заголовок".to_string())],
            },
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");
        assert!(xml.contains("Глава 1. "), "xml: {xml}");
        assert!(!xml.contains("1.."), "xml: {xml}");
    }

    #[test]
    fn table_alignment_uses_center_when_requested() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        let table = Table {
            caption: Vec::new(),
            label: None,
            source: Vec::new(),
            alignment: Some("center".to_string()),
            rows: vec![TableRow {
                cells: vec![TableCell {
                    content: vec![Inline::Text("X".to_string())],
                }],
            }],
        };
        let xml = String::from_utf8(build_table(&table, &profile).build())
            .expect("table xml should be utf8");
        assert!(xml.contains("w:jc w:val=\"center\""), "xml: {xml}");
    }

    #[test]
    fn section_heading_uses_latex_driven_indent() {
        let profile = RenderProfile::from_layout(&DocumentLayout {
            heading_indent_section_twips: Some(709),
            ..DocumentLayout::default()
        });
        let para = build_paragraph(
            &Block::Section {
                level: 2,
                number: Some("1.1".to_string()),
                label: None,
                title: vec![Inline::Text("Section".to_string())],
            },
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml should be utf8");
        assert!(xml.contains("w:left=\"709\""), "xml: {xml}");
    }

    #[test]
    fn normalize_math_text_supports_sim_and_double_dash_ranges() {
        assert_eq!(normalize_math_text("\\sim8 300--8 500"), "≈8 300–8 500");
    }

    #[test]
    fn generated_toc_paragraphs_include_chapters_and_sections() {
        let document = Document {
            blocks: vec![
                Block::Section {
                    level: 1,
                    number: None,
                    label: None,
                    title: vec![Inline::Text("ОГЛАВЛЕНИЕ".to_string())],
                },
                Block::Section {
                    level: 1,
                    number: Some("1.".to_string()),
                    label: None,
                    title: vec![Inline::Text("Глава".to_string())],
                },
                Block::Section {
                    level: 2,
                    number: Some("1.1".to_string()),
                    label: None,
                    title: vec![Inline::Text("Раздел".to_string())],
                },
            ],
            layout: DocumentLayout::default(),
            toc_entries: Vec::new(),
        };
        let profile = RenderProfile::from_layout(&document.layout);
        let paragraphs = generated_toc_paragraphs(&document, 1, &profile);
        assert_eq!(paragraphs.len(), 2);

        let first_xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        let second_xml = String::from_utf8(paragraphs[1].build()).expect("paragraph xml utf8");
        assert!(
            first_xml.contains("w:pStyle w:val=\"TOC1\""),
            "xml: {first_xml}"
        );
        assert!(
            second_xml.contains("w:pStyle w:val=\"TOC2\""),
            "xml: {second_xml}"
        );
        assert!(first_xml.contains("1. "), "xml: {first_xml}");
        assert!(second_xml.contains("1.1 "), "xml: {second_xml}");
    }

    #[test]
    fn generated_toc_paragraphs_from_entries_include_page_header_line() {
        let document = Document {
            blocks: Vec::new(),
            layout: DocumentLayout::default(),
            toc_entries: vec![
                TocEntry {
                    level: 0,
                    number: None,
                    title: "Page.".to_string(),
                    page: None,
                },
                TocEntry {
                    level: 1,
                    number: Some("1.".to_string()),
                    title: "Intro".to_string(),
                    page: Some("3".to_string()),
                },
            ],
        };
        let profile = RenderProfile::from_layout(&document.layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 2);

        let header_xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        let entry_xml = String::from_utf8(paragraphs[1].build()).expect("paragraph xml utf8");
        assert!(
            header_xml.contains("w:jc w:val=\"right\""),
            "xml: {header_xml}"
        );
        assert!(header_xml.contains("Page."), "xml: {header_xml}");
        assert!(
            entry_xml.contains("w:pStyle w:val=\"TOC1\""),
            "xml: {entry_xml}"
        );
        assert!(
            entry_xml.contains("w:tab w:val=\"right\" w:leader=\"dot\""),
            "xml: {entry_xml}"
        );
        assert!(entry_xml.contains("<w:tab "), "xml: {entry_xml}");
        assert!(entry_xml.contains(">3<"), "xml: {entry_xml}");
    }

    #[test]
    fn toc_entry_uses_latex_chapter_name_prefix_when_configured() {
        let layout = DocumentLayout {
            toc_chapter_name_prefix: Some("Глава".to_string()),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Раздел".to_string(),
                page: Some("5".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);

        let xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        assert!(xml.contains("ГЛАВА 1. "), "xml: {xml}");
        assert!(!xml.contains("1.."), "xml: {xml}");
    }

    #[test]
    fn toc_entry_uses_latex_driven_numwidth_for_hanging_indent() {
        let layout = DocumentLayout {
            toc_indent_section_twips: Some(400),
            toc_numwidth_section_twips: Some(700),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 2,
                number: Some("1.1".to_string()),
                title: "A very long section title for hanging indent behavior".to_string(),
                page: Some("12".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);

        let xml = String::from_utf8(paragraphs[0].build()).expect("paragraph xml utf8");
        assert!(xml.contains("w:left=\"1100\""), "xml: {xml}");
        assert!(xml.contains("w:hanging=\"700\""), "xml: {xml}");
    }

    #[test]
    fn linebreak_inline_renders_text_wrapping_break() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        let para = build_styled_body_paragraph(
            &[
                Inline::Text("Line 1".to_string()),
                Inline::LineBreak,
                Inline::Text("Line 2".to_string()),
            ],
            &crate::model::ParagraphStyle::default(),
            &profile,
        );
        let xml = String::from_utf8(para.build()).expect("paragraph xml utf8");
        assert!(xml.contains("w:br w:type=\"textWrapping\""), "xml: {xml}");
    }

    #[test]
    fn resolve_figure_path_uses_images_fallback_and_extension_guess() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_figure_path_{unique}"));
        let images_dir = root.join("images").join("part2");
        std::fs::create_dir_all(&images_dir).expect("failed to create images dir");
        let image_path = images_dir.join("chart.png");
        std::fs::write(&image_path, b"fake").expect("failed to write image");

        let default_paths = vec![
            "images".to_string(),
            "figures".to_string(),
            "img".to_string(),
        ];
        let resolved = resolve_figure_path("part2/chart", Some(&root), &default_paths);
        assert_eq!(resolved, Some(image_path.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_figure_path_accepts_absolute_path() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferritex_figure_abs_{unique}"));
        std::fs::create_dir_all(&root).expect("failed to create dir");
        let image_path = root.join("figure.jpg");
        std::fs::write(&image_path, b"fake").expect("failed to write image");

        let absolute = image_path.to_string_lossy().to_string();
        let resolved = resolve_figure_path(&absolute, None, &[]);
        assert_eq!(resolved, Some(image_path.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Task 3: new TOC fields — fallback defaults ────────────────────

    #[test]
    fn toc_fallback_defaults_new_fields() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        assert!(
            profile.toc_chapter_entry_bold,
            "default chapter entry should be bold"
        );
        assert!(
            profile.toc_chapter_page_bold,
            "default chapter page number should be bold"
        );
        assert_eq!(profile.toc_aftersnum_chapter, "");
        assert_eq!(profile.toc_aftersnum_section, "");
        assert_eq!(profile.toc_aftersnum_subsection, "");
        assert_eq!(profile.toc_aftersnum_subsubsection, "");
    }

    #[test]
    fn toc_entry_chapter_non_bold_when_toc_chapter_entry_bold_false() {
        let layout = DocumentLayout {
            toc_chapter_entry_bold: Some(false),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Introduction".to_string(),
                page: Some("5".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);
        let xml = String::from_utf8(paragraphs[0].build()).expect("xml utf8");
        // When non-bold is requested, w:b w:val="false" must appear in the title run.
        assert!(
            xml.contains("w:b w:val=\"false\""),
            "expected non-bold chapter entry, xml: {xml}"
        );
    }

    #[test]
    fn toc_entry_uses_aftersnum_chapter_separator() {
        let layout = DocumentLayout {
            toc_aftersnum_chapter: Some(". ".to_string()),
            ..DocumentLayout::default()
        };
        let document = Document {
            blocks: Vec::new(),
            layout: layout.clone(),
            toc_entries: vec![TocEntry {
                level: 1,
                number: Some("1.".to_string()),
                title: "Sample chapter".to_string(),
                page: Some("3".to_string()),
            }],
        };
        let profile = RenderProfile::from_layout(&layout);
        let paragraphs = generated_toc_paragraphs(&document, 0, &profile);
        assert_eq!(paragraphs.len(), 1);
        let xml = String::from_utf8(paragraphs[0].build()).expect("xml utf8");
        // Number prefix should use aftersnum ". " (rendered as "1. ").
        assert!(
            xml.contains("1. "),
            "expected '1. ' with aftersnum separator, xml: {xml}"
        );
    }

    // ── List indent geometry tests ────────────────────────────────────────────

    #[test]
    fn list_indent_defaults_use_body_first_line_indent() {
        let mut layout = DocumentLayout::default();
        layout.body_first_line_indent_twips = Some(709);
        let profile = RenderProfile::from_layout(&layout);
        // list_left should equal body first-line indent (parindent).
        assert_eq!(profile.list_left_indent_twips, 709);
        // list_hanging = labelsep + labelwidth (both default to ~142 twips).
        assert!(
            profile.list_hanging_indent_twips > 0,
            "hanging indent should be positive"
        );
    }

    #[test]
    fn list_bullet_char_fallback_is_bullet() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.list_bullet_char, "•");
    }

    #[test]
    fn list_bullet_char_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.list_bullet_char = Some("–".to_string());
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.list_bullet_char, "–");
    }

    // ── Source vspace tests ───────────────────────────────────────────────────

    #[test]
    fn source_vspace_defaults() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(
            profile.source_vspace_table_twips,
            DEFAULT_SOURCE_VSPACE_TABLE_TWIPS
        );
        assert_eq!(
            profile.source_vspace_figure_twips,
            DEFAULT_SOURCE_VSPACE_FIGURE_TWIPS
        );
    }

    #[test]
    fn source_vspace_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.source_vspace_table_twips = Some(100);
        layout.source_vspace_figure_twips = Some(50);
        let profile = RenderProfile::from_layout(&layout);
        assert_eq!(profile.source_vspace_table_twips, 100);
        assert_eq!(profile.source_vspace_figure_twips, 50);
    }

    #[test]
    fn caption_indent_defaults_to_zero_for_figure_and_table() {
        let profile = RenderProfile::from_layout(&DocumentLayout::default());
        assert_eq!(profile.caption_indent_twips_figure, 0);
        assert_eq!(profile.caption_indent_twips_table, 0);
    }

    // ── Title page suppression tests ──────────────────────────────────────────

    #[test]
    fn title_page_suppress_defaults_false() {
        let layout = DocumentLayout::default();
        let profile = RenderProfile::from_layout(&layout);
        assert!(!profile.title_page_suppress_number);
    }

    #[test]
    fn title_page_suppress_from_layout() {
        let mut layout = DocumentLayout::default();
        layout.title_page_suppress_number = Some(true);
        let profile = RenderProfile::from_layout(&layout);
        assert!(profile.title_page_suppress_number);
    }
