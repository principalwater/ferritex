/// DOCX backend crate facade.
pub mod docx {
    pub use ferritex_renderer_docx::{render_docx, render_docx_with_context};
}

/// PDF backend crate facade.
pub mod pdf {
    pub use ferritex_renderer_pdf::render_pdf_with_context;
}

/// Markdown backend crate facade.
pub mod md {
    pub use ferritex_renderer_md::render_md_with_context;
}
