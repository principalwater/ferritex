use std::{
    collections::HashSet,
    ffi::OsString,
    fs::File,
    io::{IsTerminal, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use ferritex_core::model::Document;
use flate2::read::GzDecoder;
use sha1::{Digest, Sha1};
use tar::Archive as TarArchive;
use tectonic::{
    config::PersistentConfig,
    driver::{OutputFormat, ProcessingSessionBuilder},
    status::{MessageKind, StatusBackend},
};
#[cfg(windows)]
use zip::ZipArchive;

const PDF_COMPAT_SHIMS: &str = r#"\makeatletter
\@ifundefined{MakeUppercaseUnsupportedInPdfStrings}{\providecommand\MakeUppercaseUnsupportedInPdfStrings[1]{#1}}{}
\@ifundefined{MakeLowercaseUnsupportedInPdfStrings}{\providecommand\MakeLowercaseUnsupportedInPdfStrings[1]{#1}}{}
\makeatother
"#;
const ENV_PDF_BIBER_BIN_DIR: &str = "FERRITEX_PDF_BIBER_BIN_DIR";
const ENV_PDF_BIBER_BIN_DIRS: &str = "FERRITEX_PDF_BIBER_BIN_DIRS";
const ENV_PDF_BIBER_AUTO_INSTALL: &str = "FERRITEX_PDF_BIBER_AUTO_INSTALL";
const ENV_PDF_BIBER_CACHE_DIR: &str = "FERRITEX_PDF_BIBER_CACHE_DIR";
const BIBER_MISMATCH_MARKER: &str = "biber/biblatex compatibility mismatch detected";
const AUTO_BIBER_SUPPORTED_MATRIX: &str =
    "BCF 3.8 -> biber 2.17 (platforms: macos-universal, linux-x86_64, windows-x86_64)";
const SOURCEFORGE_BIBER_BASE_URL: &str = "https://sourceforge.net/projects/biblatex-biber/files";
const AUTOINSTALL_USER_AGENT: &str = "ferritex/pdf-biber-autoinstall";
const MAX_AUTOINSTALL_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;

#[cfg(windows)]
const BIBER_BINARY_NAMES: [&str; 2] = ["biber.exe", "biber"];
#[cfg(not(windows))]
const BIBER_BINARY_NAMES: [&str; 1] = ["biber"];

/// Bibliography tool resolution mode for PDF builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BiberResolutionMode {
    /// Fail immediately on the first selected `biber` candidate.
    Strict,
    /// Retry with alternative `biber` candidates when a BCF compatibility mismatch is detected.
    #[default]
    Auto,
}

/// Policy for handling missing or incompatible external runtime tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInstallPolicy {
    /// Ask the user before installing a compatible tool.
    Ask,
    /// Install compatible tools automatically when possible.
    Auto,
    /// Never install tools automatically; fail with guidance.
    Never,
}

#[derive(Debug, Clone)]
struct BiberAttemptPlanItem {
    biber_bin_dir: Option<PathBuf>,
    label: String,
}

#[derive(Debug, Clone, Copy)]
enum AutoBiberArchiveFormat {
    TarGz,
    #[cfg(windows)]
    Zip,
}

#[derive(Debug, Clone, Copy)]
struct AutoBiberAsset {
    biber_version: &'static str,
    bcf_version: &'static str,
    platform_id: &'static str,
    sourceforge_platform_dir: &'static str,
    archive_name: &'static str,
    archive_sha1: &'static str,
    archive_format: AutoBiberArchiveFormat,
    binary_name: &'static str,
}

#[derive(Debug, Clone)]
struct AutoBiberInstall {
    biber_bin_dir: PathBuf,
    biber_version: &'static str,
    bcf_version: &'static str,
}

#[derive(Debug)]
struct PathEnvGuard {
    original_path: Option<OsString>,
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        if let Some(path) = self.original_path.as_ref() {
            // SAFETY: ferritex runs the build pipeline in a single-process CLI flow;
            // PATH is restored immediately after the PDF session.
            unsafe { std::env::set_var("PATH", path) };
        } else {
            // SAFETY: ferritex runs the build pipeline in a single-process CLI flow;
            // PATH is restored immediately after the PDF session.
            unsafe { std::env::remove_var("PATH") };
        }
    }
}

#[derive(Default)]
struct CapturingStatusBackend {
    messages: Vec<String>,
    error_logs: Vec<String>,
}

impl CapturingStatusBackend {
    fn messages_excerpt(&self) -> Option<String> {
        if self.messages.is_empty() {
            return None;
        }

        Some(
            self.messages
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | "),
        )
    }

    fn error_logs_excerpt(&self) -> Option<String> {
        self.error_logs
            .iter()
            .find(|entry| !entry.trim().is_empty())
            .map(|entry| external_tool_excerpt(entry))
    }
}

impl StatusBackend for CapturingStatusBackend {
    fn report(
        &mut self,
        kind: MessageKind,
        args: std::fmt::Arguments,
        err: Option<&anyhow::Error>,
    ) {
        if kind == MessageKind::Note {
            return;
        }

        let mut message = format!("{kind:?}: {args}");
        if let Some(err) = err {
            let chain = err
                .chain()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            if !chain.is_empty() {
                message.push_str(" | caused by: ");
                message.push_str(&chain);
            }
        }
        self.messages.push(message);
    }

    fn dump_error_logs(&mut self, output: &[u8]) {
        if output.is_empty() {
            return;
        }

        self.error_logs
            .push(String::from_utf8_lossy(output).into_owned());
    }
}

/// Render a document to PDF.
///
/// The PDF backend uses tectonic driver APIs as the canonical runtime path
/// for parity-oriented output.
pub fn render_pdf_with_context(
    _document: &Document,
    output: &Path,
    input_context: Option<&Path>,
    biber_bin_dir_override: Option<&Path>,
    biber_resolution_mode: BiberResolutionMode,
    tool_install_policy: ToolInstallPolicy,
) -> Result<()> {
    let input_path = input_context.ok_or_else(|| {
        anyhow!("PDF rendering requires input context path to resolve LaTeX includes and assets")
    })?;
    compile_pdf_with_tectonic(
        input_path,
        output,
        biber_bin_dir_override,
        biber_resolution_mode,
        tool_install_policy,
    )?;
    let metadata = std::fs::metadata(output)
        .map_err(|error| anyhow!("failed to stat generated PDF {}: {error}", output.display()))?;
    if metadata.len() == 0 {
        return Err(anyhow!(
            "tectonic PDF build produced an empty file at {}",
            output.display()
        ));
    }
    Ok(())
}

fn compile_pdf_with_tectonic(
    input_path: &Path,
    output_path: &Path,
    biber_bin_dir_override: Option<&Path>,
    biber_resolution_mode: BiberResolutionMode,
    tool_install_policy: ToolInstallPolicy,
) -> Result<()> {
    let resolved_override = resolve_biber_bin_dir(biber_bin_dir_override);
    let mut attempts =
        build_biber_attempt_plan(resolved_override.clone(), biber_resolution_mode, input_path);
    let mut attempted = Vec::new();
    let mut idx = 0usize;
    let mut autoinstall_used = false;

    while let Some(attempt) = attempts.get(idx).cloned() {
        match compile_pdf_with_tectonic_attempt(
            input_path,
            output_path,
            attempt.biber_bin_dir.as_deref(),
        ) {
            Ok(()) => {
                if idx > 0 {
                    log::warn!(
                        "tectonic PDF build recovered with alternate biber candidate '{}'",
                        attempt.label
                    );
                }
                return Ok(());
            }
            Err(error) => {
                let rendered = format!("{error:#}");
                let mismatch = parse_biber_mismatch_versions(&rendered);
                let has_more_attempts = idx + 1 < attempts.len();
                let mismatch_detected = rendered.contains(BIBER_MISMATCH_MARKER);
                attempted.push(format!(
                    "{} => {}",
                    attempt.label,
                    first_non_empty_line(&rendered)
                ));
                let should_retry = biber_resolution_mode == BiberResolutionMode::Auto
                    && resolved_override.is_none()
                    && mismatch_detected
                    && has_more_attempts;
                if should_retry {
                    log::warn!(
                        "tectonic PDF build hit bibliography compatibility mismatch with '{}'; retrying next candidate",
                        attempt.label
                    );
                    idx += 1;
                    continue;
                }

                let should_try_autoinstall = biber_resolution_mode == BiberResolutionMode::Auto
                    && resolved_override.is_none()
                    && !has_more_attempts
                    && mismatch_detected
                    && !autoinstall_used;
                if should_try_autoinstall {
                    autoinstall_used = true;
                    if let Some((observed_bcf, expected_bcf)) = mismatch {
                        match evaluate_biber_auto_install_policy(
                            tool_install_policy,
                            &observed_bcf,
                            &expected_bcf,
                        )? {
                            AutoInstallPermission::Allowed => {
                                match ensure_auto_installed_biber_for_bcf(&observed_bcf) {
                                    Ok(Some(install)) => {
                                        let label = format!(
                                            "auto-installed biber {} for BCF {}",
                                            install.biber_version, install.bcf_version
                                        );
                                        attempts.push(BiberAttemptPlanItem {
                                            biber_bin_dir: Some(install.biber_bin_dir),
                                            label: label.clone(),
                                        });
                                        log::warn!("tectonic PDF build is retrying with {}", label);
                                        idx += 1;
                                        continue;
                                    }
                                    Ok(None) => {
                                        let matrix_guidance =
                                            auto_biber_matrix_guidance(&observed_bcf);
                                        log::warn!(
                                            "no built-in biber auto-install asset available: {}",
                                            matrix_guidance
                                        );
                                    }
                                    Err(install_error) => {
                                        log::warn!(
                                            "failed to auto-install compatible biber for BCF {}: {install_error}",
                                            observed_bcf
                                        );
                                    }
                                }
                            }
                            AutoInstallPermission::Denied(reason) => {
                                let rendered =
                                    with_biber_resolution_attempts(rendered, attempted.as_slice());
                                return Err(anyhow!("{rendered} | {reason}"));
                            }
                        }
                    }
                }

                if attempted.len() > 1 {
                    return Err(anyhow!(with_biber_resolution_attempts(
                        rendered,
                        attempted.as_slice()
                    )));
                }
                return Err(error);
            }
        }
    }

    Err(anyhow!(
        "tectonic PDF build failed before running any biber resolution attempt"
    ))
}

fn with_biber_resolution_attempts(rendered: String, attempted: &[String]) -> String {
    if attempted.len() > 1 {
        format!(
            "{rendered}\nbiber resolution attempts: {}",
            attempted.join(" | ")
        )
    } else {
        rendered
    }
}

fn compile_pdf_with_tectonic_attempt(
    input_path: &Path,
    output_path: &Path,
    biber_bin_dir_override: Option<&Path>,
) -> Result<()> {
    let _path_guard = apply_biber_bin_dir_override(biber_bin_dir_override)?;
    let input_root = input_path.parent().unwrap_or_else(|| Path::new("."));
    let output_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let input_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow!(
                "failed to derive TeX input name from {}",
                input_path.display()
            )
        })?;
    let pdf_name = output_name_for_extension(input_name, "pdf");
    let primary_input = build_pdf_primary_input(input_name);

    let mut status = CapturingStatusBackend::default();
    let config = PersistentConfig::open(false)
        .map_err(|error| anyhow!("failed to open default tectonic config: {error}"))?;
    let bundle = config
        .default_bundle(false, &mut status)
        .map_err(|error| anyhow!("failed to load tectonic default bundle: {error}"))?;
    let format_cache_path = config
        .format_cache_path()
        .map_err(|error| anyhow!("failed to resolve tectonic format cache path: {error}"))?;

    let mut builder = ProcessingSessionBuilder::default();
    builder
        .bundle(bundle)
        .primary_input_buffer(primary_input.as_bytes())
        .tex_input_name(input_name)
        .filesystem_root(input_root)
        .output_dir(output_dir)
        .format_name("latex")
        .format_cache_path(format_cache_path)
        .output_format(OutputFormat::Pdf)
        .keep_logs(true)
        .keep_intermediates(false)
        .print_stdout(false)
        .build_date_from_env(false);

    let mut session = builder
        .create(&mut status)
        .map_err(|error| anyhow!("failed to create tectonic PDF session: {error}"))?;
    let run_result = session.run(&mut status);
    let mut files = session.into_file_data();

    if let Err(error) = run_result {
        let expected_log_name = output_name_for_extension(input_name, "log");
        let log_text = files
            .remove(&expected_log_name)
            .map(|file| String::from_utf8_lossy(&file.data).into_owned())
            .or_else(|| read_text_if_exists(&output_dir.join(&expected_log_name)))
            .or_else(|| {
                files
                    .iter()
                    .find(|(name, _)| name.ends_with(".log"))
                    .map(|(_, file)| String::from_utf8_lossy(&file.data).into_owned())
            })
            .unwrap_or_else(|| "<no tectonic log artifact captured>".to_string());
        let biber_log_excerpt = files
            .iter()
            .find(|(name, _)| name.ends_with(".blg"))
            .map(|(_, file)| String::from_utf8_lossy(&file.data).into_owned())
            .or_else(|| {
                read_text_if_exists(&output_dir.join(output_name_for_extension(input_name, "blg")))
            })
            .map(|blg_text| probe_log_excerpt(&blg_text));
        let external_tool_excerpt = status.error_logs_excerpt();
        let external_tool_messages = status.messages_excerpt();
        let biber_version_hint =
            biber_bcf_version_mismatch_hint(&external_tool_excerpt, &status.error_logs);
        if let Ok(log_dump_path) = std::env::var("FERRITEX_TECTONIC_PDF_LOG_DUMP")
            && let Err(write_error) = std::fs::write(&log_dump_path, &log_text)
        {
            log::warn!(
                "failed to write tectonic PDF log dump to {}: {write_error}",
                log_dump_path
            );
        }
        let error_chain = format_error_chain(&error);
        let mut details = vec![format!("log excerpt: {}", probe_log_excerpt(&log_text))];
        if let Some(blg_excerpt) = biber_log_excerpt
            && !blg_excerpt.is_empty()
        {
            details.push(format!("biber log excerpt: {blg_excerpt}"));
        }
        if let Some(external_excerpt) = external_tool_excerpt
            && !external_excerpt.is_empty()
        {
            details.push(format!("external tool excerpt: {external_excerpt}"));
        }
        if let Some(messages) = external_tool_messages
            && !messages.is_empty()
        {
            details.push(format!("tectonic status: {messages}"));
        }
        if let Some(hint) = biber_version_hint {
            details.push(hint);
        }
        return Err(anyhow!(
            "tectonic PDF build failed for {}: {}. {}",
            input_path.display(),
            error_chain,
            details.join(" | ")
        ));
    }

    let generated_pdf_path = output_dir.join(&pdf_name);
    if !generated_pdf_path.exists() {
        if let Some(file) = files.remove(&pdf_name) {
            std::fs::write(output_path, file.data).map_err(|error| {
                anyhow!(
                    "failed to write PDF artifact {} from tectonic in-memory output: {error}",
                    output_path.display()
                )
            })?;
            return Ok(());
        }
        return Err(anyhow!(
            "tectonic PDF build did not emit expected artifact '{}' for {}",
            pdf_name,
            input_path.display()
        ));
    }

    if generated_pdf_path != output_path {
        std::fs::rename(&generated_pdf_path, output_path).or_else(|rename_error| {
            std::fs::copy(&generated_pdf_path, output_path)
                .map(|_| ())
                .map_err(|copy_error| {
                    anyhow!(
                        "failed to move generated PDF from {} to {}: rename error: {rename_error}; copy error: {copy_error}",
                        generated_pdf_path.display(),
                        output_path.display()
                    )
                })
        })?;
    }

    Ok(())
}

fn output_name_for_extension(input_name: &str, extension: &str) -> String {
    Path::new(input_name)
        .with_extension(extension)
        .to_string_lossy()
        .into_owned()
}

fn build_biber_attempt_plan(
    resolved_override: Option<PathBuf>,
    biber_resolution_mode: BiberResolutionMode,
    input_path: &Path,
) -> Vec<BiberAttemptPlanItem> {
    if let Some(override_dir) = resolved_override {
        return vec![BiberAttemptPlanItem {
            label: format!("explicit override {}", override_dir.display()),
            biber_bin_dir: Some(override_dir),
        }];
    }

    let mut attempts = Vec::new();
    let mut seen = HashSet::new();

    if biber_resolution_mode == BiberResolutionMode::Auto
        && let Some(preferred) = preferred_cached_biber_attempt_from_sidecar(input_path)
    {
        log::info!(
            "PDF bibliography auto-mode: prioritizing {}",
            preferred.label
        );
        if let Some(dir) = preferred.biber_bin_dir.as_ref() {
            seen.insert(dir.clone());
        }
        attempts.push(preferred);
    }

    attempts.push(BiberAttemptPlanItem {
        label: "PATH default".to_string(),
        biber_bin_dir: None,
    });
    if biber_resolution_mode != BiberResolutionMode::Auto {
        return attempts;
    }

    let path_dirs = discover_biber_bin_dirs_from_path();
    if let Some(first) = path_dirs.first() {
        seen.insert(first.clone());
    }

    for dir in discover_biber_bin_dirs_from_env()
        .into_iter()
        .chain(path_dirs.into_iter())
    {
        if seen.insert(dir.clone()) {
            attempts.push(BiberAttemptPlanItem {
                label: format!("candidate {}", dir.display()),
                biber_bin_dir: Some(dir),
            });
        }
    }

    attempts
}

fn preferred_cached_biber_attempt_from_sidecar(input_path: &Path) -> Option<BiberAttemptPlanItem> {
    let sidecar_version = read_bcf_controlfile_version(input_path)?;
    let cached = find_cached_auto_biber_for_bcf(&sidecar_version)?;
    Some(BiberAttemptPlanItem {
        label: format!(
            "cached auto-installed biber {} for BCF {}",
            cached.biber_version, cached.bcf_version
        ),
        biber_bin_dir: Some(cached.biber_bin_dir),
    })
}

fn read_bcf_controlfile_version(input_path: &Path) -> Option<String> {
    let bcf_path = input_path.with_extension("bcf");
    let bcf_text = read_text_if_exists(&bcf_path)?;
    parse_bcf_controlfile_version(&bcf_text)
}

fn parse_bcf_controlfile_version(bcf_text: &str) -> Option<String> {
    parse_version_after_marker(bcf_text, "controlfile version=\"")
}

fn auto_biber_matrix_guidance(observed_bcf: &str) -> String {
    if let Some(asset) = resolve_auto_biber_asset_for_bcf(observed_bcf) {
        return format!(
            "built-in auto-install supports BCF {} -> biber {} on platform {}",
            asset.bcf_version, asset.biber_version, asset.platform_id
        );
    }

    format!(
        "built-in auto-install currently supports only {AUTO_BIBER_SUPPORTED_MATRIX}; observed BCF {} has no bundled auto-install asset for current platform {}",
        observed_bcf,
        current_platform_id()
    )
}

enum AutoInstallPermission {
    Allowed,
    Denied(String),
}

fn evaluate_biber_auto_install_policy(
    policy: ToolInstallPolicy,
    observed_bcf: &str,
    expected_bcf: &str,
) -> Result<AutoInstallPermission> {
    if !biber_auto_install_enabled() {
        return Ok(AutoInstallPermission::Denied(format!(
            "compatible biber auto-install is disabled via {ENV_PDF_BIBER_AUTO_INSTALL}=0; install a compatible biber manually and rerun ferritex"
        )));
    }

    match policy {
        ToolInstallPolicy::Auto => Ok(AutoInstallPermission::Allowed),
        ToolInstallPolicy::Never => Ok(AutoInstallPermission::Denied(
            "automatic tool installation is disabled by --tool-install-policy never; install a compatible biber manually and rerun ferritex (or rerun with --tool-install-policy auto)".to_string(),
        )),
        ToolInstallPolicy::Ask => {
            if !interactive_prompt_supported() {
                return Ok(AutoInstallPermission::Denied(
                    "cannot prompt for compatible biber installation in non-interactive mode; rerun with --tool-install-policy auto to allow automatic installation, or install a compatible biber manually and rerun ferritex".to_string(),
                ));
            }

            let install_confirmed =
                prompt_user_for_biber_auto_install(observed_bcf, expected_bcf)?;
            if install_confirmed {
                Ok(AutoInstallPermission::Allowed)
            } else {
                Ok(AutoInstallPermission::Denied(
                    "compatible biber installation was declined; install a compatible biber manually and rerun ferritex".to_string(),
                ))
            }
        }
    }
}

fn interactive_prompt_supported() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn prompt_user_for_biber_auto_install(observed_bcf: &str, expected_bcf: &str) -> Result<bool> {
    let mut stderr = std::io::stderr();
    writeln!(
        stderr,
        "ferritex detected biber/biblatex mismatch (BCF {observed_bcf}, biber expects {expected_bcf})."
    )
    .map_err(|error| anyhow!("failed to write interactive install prompt: {error}"))?;
    write!(
        stderr,
        "Install a compatible biber into ferritex cache now? [y/N]: "
    )
    .map_err(|error| anyhow!("failed to write interactive install prompt: {error}"))?;
    stderr
        .flush()
        .map_err(|error| anyhow!("failed to flush interactive install prompt: {error}"))?;

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| anyhow!("failed to read interactive install prompt response: {error}"))?;

    let normalized = answer.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

fn biber_auto_install_enabled() -> bool {
    std::env::var(ENV_PDF_BIBER_AUTO_INSTALL)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "no" | "off")
        })
        .unwrap_or(true)
}

fn ensure_auto_installed_biber_for_bcf(observed_bcf: &str) -> Result<Option<AutoBiberInstall>> {
    let Some(asset) = resolve_auto_biber_asset_for_bcf(observed_bcf) else {
        return Ok(None);
    };

    let cache_root = resolve_biber_cache_root()?;
    let archive_dir = cache_root.join("archives");
    let install_dir = cache_root
        .join(format!("biber-{}", asset.biber_version))
        .join(asset.platform_id);
    let installed_binary = install_dir.join(asset.binary_name);

    if installed_binary.is_file() {
        return Ok(Some(AutoBiberInstall {
            biber_bin_dir: install_dir,
            biber_version: asset.biber_version,
            bcf_version: asset.bcf_version,
        }));
    }

    std::fs::create_dir_all(&archive_dir).map_err(|error| {
        anyhow!(
            "failed to create biber archive cache directory {}: {error}",
            archive_dir.display()
        )
    })?;
    std::fs::create_dir_all(&install_dir).map_err(|error| {
        anyhow!(
            "failed to create biber install directory {}: {error}",
            install_dir.display()
        )
    })?;

    let archive_path = archive_dir.join(asset.archive_name);
    ensure_downloaded_archive(&archive_path, asset)?;
    install_biber_from_archive(&archive_path, &install_dir, asset)?;

    if !installed_binary.is_file() {
        return Err(anyhow!(
            "auto-installed biber archive did not provide expected binary {}",
            installed_binary.display()
        ));
    }

    Ok(Some(AutoBiberInstall {
        biber_bin_dir: install_dir,
        biber_version: asset.biber_version,
        bcf_version: asset.bcf_version,
    }))
}

fn find_cached_auto_biber_for_bcf(observed_bcf: &str) -> Option<AutoBiberInstall> {
    let asset = resolve_auto_biber_asset_for_bcf(observed_bcf)?;
    let cache_root = resolve_biber_cache_root().ok()?;
    let install_dir = cache_root
        .join(format!("biber-{}", asset.biber_version))
        .join(asset.platform_id);
    let installed_binary = install_dir.join(asset.binary_name);
    if installed_binary.is_file() {
        Some(AutoBiberInstall {
            biber_bin_dir: install_dir,
            biber_version: asset.biber_version,
            bcf_version: asset.bcf_version,
        })
    } else {
        None
    }
}

fn resolve_auto_biber_asset_for_bcf(observed_bcf: &str) -> Option<AutoBiberAsset> {
    if observed_bcf != "3.8" {
        return None;
    }

    resolve_auto_biber_asset_for_current_platform()
}

#[cfg(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
fn current_platform_id() -> &'static str {
    "macos-universal"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn current_platform_id() -> &'static str {
    "linux-x86_64"
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn current_platform_id() -> &'static str {
    "windows-x86_64"
}

#[cfg(not(any(
    all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn current_platform_id() -> &'static str {
    "unsupported-platform"
}

#[cfg(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
fn resolve_auto_biber_asset_for_current_platform() -> Option<AutoBiberAsset> {
    Some(AutoBiberAsset {
        biber_version: "2.17",
        bcf_version: "3.8",
        platform_id: "macos-universal",
        sourceforge_platform_dir: "MacOS",
        archive_name: "biber-darwin_universal.tar.gz",
        archive_sha1: "0050ffda66a97aa83aa2d7e615c23ee3e12a7b63",
        archive_format: AutoBiberArchiveFormat::TarGz,
        binary_name: "biber",
    })
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn resolve_auto_biber_asset_for_current_platform() -> Option<AutoBiberAsset> {
    Some(AutoBiberAsset {
        biber_version: "2.17",
        bcf_version: "3.8",
        platform_id: "linux-x86_64",
        sourceforge_platform_dir: "Linux",
        archive_name: "biber-linux_x86_64.tar.gz",
        archive_sha1: "a149dc16f6006dc1970cff7681295a46f0fd3ce2",
        archive_format: AutoBiberArchiveFormat::TarGz,
        binary_name: "biber",
    })
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn resolve_auto_biber_asset_for_current_platform() -> Option<AutoBiberAsset> {
    Some(AutoBiberAsset {
        biber_version: "2.17",
        bcf_version: "3.8",
        platform_id: "windows-x86_64",
        sourceforge_platform_dir: "Windows",
        archive_name: "biber-MSWIN64.zip",
        archive_sha1: "1f00870c645f96c61a4f586f13863e42c3fb83b0",
        archive_format: AutoBiberArchiveFormat::Zip,
        binary_name: "biber.exe",
    })
}

#[cfg(not(any(
    all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn resolve_auto_biber_asset_for_current_platform() -> Option<AutoBiberAsset> {
    None
}

fn resolve_biber_cache_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_PDF_BIBER_CACHE_DIR) {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            return Err(anyhow!(
                "{ENV_PDF_BIBER_CACHE_DIR} is set but empty; provide a valid cache directory path"
            ));
        }
        return Ok(path);
    }

    let config = PersistentConfig::open(false).map_err(|error| {
        anyhow!("failed to open default tectonic config for biber cache: {error}")
    })?;
    let format_cache_path = config.format_cache_path().map_err(|error| {
        anyhow!("failed to resolve tectonic format cache path for biber cache: {error}")
    })?;
    let cache_base = format_cache_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(format_cache_path);
    Ok(cache_base.join("ferritex").join("pdf-biber"))
}

fn ensure_downloaded_archive(archive_path: &Path, asset: AutoBiberAsset) -> Result<()> {
    if archive_path.is_file() {
        if verify_file_sha1(archive_path, asset.archive_sha1)? {
            return Ok(());
        }
        std::fs::remove_file(archive_path).map_err(|error| {
            anyhow!(
                "cached biber archive {} has unexpected checksum and could not be removed: {error}",
                archive_path.display()
            )
        })?;
    }

    let response = ureq::get(&auto_biber_download_url(asset))
        .set("User-Agent", AUTOINSTALL_USER_AGENT)
        .call()
        .map_err(|error| anyhow!("failed to download compatible biber archive: {error}"))?;
    let mut stream = response.into_reader();
    let tmp_path = archive_path.with_extension("download");
    let mut file = File::create(&tmp_path).map_err(|error| {
        anyhow!(
            "failed to create temporary archive file {}: {error}",
            tmp_path.display()
        )
    })?;

    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 16 * 1024];
    let mut total_bytes = 0usize;
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| anyhow!("failed while downloading biber archive stream: {error}"))?;
        if read == 0 {
            break;
        }
        total_bytes += read;
        if total_bytes > MAX_AUTOINSTALL_ARCHIVE_BYTES {
            return Err(anyhow!(
                "downloaded biber archive exceeded safety limit of {} bytes",
                MAX_AUTOINSTALL_ARCHIVE_BYTES
            ));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|error| {
            anyhow!(
                "failed to write temporary biber archive {}: {error}",
                tmp_path.display()
            )
        })?;
    }
    file.flush().map_err(|error| {
        anyhow!(
            "failed to flush temporary biber archive {}: {error}",
            tmp_path.display()
        )
    })?;

    let checksum = format!("{:x}", hasher.finalize());
    if checksum != asset.archive_sha1 {
        return Err(anyhow!(
            "downloaded biber archive checksum mismatch: expected {}, got {}",
            asset.archive_sha1,
            checksum
        ));
    }

    std::fs::rename(&tmp_path, archive_path).map_err(|error| {
        anyhow!(
            "failed to move downloaded biber archive from {} to {}: {error}",
            tmp_path.display(),
            archive_path.display()
        )
    })?;
    Ok(())
}

fn verify_file_sha1(path: &Path, expected_sha1: &str) -> Result<bool> {
    let mut file = File::open(path).map_err(|error| {
        anyhow!(
            "failed to open cached biber archive {}: {error}",
            path.display()
        )
    })?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            anyhow!(
                "failed to read cached biber archive {}: {error}",
                path.display()
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected_sha1)
}

fn install_biber_from_archive(
    archive_path: &Path,
    install_dir: &Path,
    asset: AutoBiberAsset,
) -> Result<()> {
    let output_path = install_dir.join(asset.binary_name);
    let temp_output_path = install_dir.join(format!("{}.tmp", asset.binary_name));
    if output_path.is_file() {
        return Ok(());
    }

    match asset.archive_format {
        AutoBiberArchiveFormat::TarGz => {
            extract_binary_from_tar_gz(archive_path, &temp_output_path, asset.binary_name)?
        }
        #[cfg(windows)]
        AutoBiberArchiveFormat::Zip => {
            extract_binary_from_zip(archive_path, &temp_output_path, asset.binary_name)?
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(&temp_output_path)
            .map_err(|error| anyhow!("failed to stat {}: {error}", temp_output_path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_output_path, perms).map_err(|error| {
            anyhow!(
                "failed to mark auto-installed biber executable {}: {error}",
                temp_output_path.display()
            )
        })?;
    }

    std::fs::rename(&temp_output_path, &output_path).map_err(|error| {
        anyhow!(
            "failed to place auto-installed biber from {} to {}: {error}",
            temp_output_path.display(),
            output_path.display()
        )
    })?;
    Ok(())
}

fn extract_binary_from_tar_gz(
    archive_path: &Path,
    output_path: &Path,
    binary_name: &str,
) -> Result<()> {
    let file = File::open(archive_path).map_err(|error| {
        anyhow!(
            "failed to open biber archive {}: {error}",
            archive_path.display()
        )
    })?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);

    let mut found = false;
    for entry in archive
        .entries()
        .map_err(|error| anyhow!("failed to read biber tar archive entries: {error}"))?
    {
        let mut entry =
            entry.map_err(|error| anyhow!("failed to read biber tar entry: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| anyhow!("failed to read biber tar entry path: {error}"))?;
        if path.file_name().and_then(|name| name.to_str()) != Some(binary_name) {
            continue;
        }
        let mut out = File::create(output_path)
            .map_err(|error| anyhow!("failed to create {}: {error}", output_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| anyhow!("failed to unpack biber binary: {error}"))?;
        out.flush()
            .map_err(|error| anyhow!("failed to flush {}: {error}", output_path.display()))?;
        found = true;
        break;
    }

    if !found {
        return Err(anyhow!(
            "biber archive {} does not contain expected binary {}",
            archive_path.display(),
            binary_name
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn extract_binary_from_zip(
    archive_path: &Path,
    output_path: &Path,
    binary_name: &str,
) -> Result<()> {
    let file = File::open(archive_path).map_err(|error| {
        anyhow!(
            "failed to open biber archive {}: {error}",
            archive_path.display()
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        anyhow!(
            "failed to open biber zip archive {}: {error}",
            archive_path.display()
        )
    })?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| anyhow!("failed to read biber zip entry {index}: {error}"))?;
        let entry_path = Path::new(entry.name());
        if entry_path.file_name().and_then(|name| name.to_str()) != Some(binary_name) {
            continue;
        }

        let mut out = File::create(output_path)
            .map_err(|error| anyhow!("failed to create {}: {error}", output_path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| anyhow!("failed to unpack biber binary from zip: {error}"))?;
        out.flush()
            .map_err(|error| anyhow!("failed to flush {}: {error}", output_path.display()))?;
        return Ok(());
    }

    Err(anyhow!(
        "biber archive {} does not contain expected binary {}",
        archive_path.display(),
        binary_name
    ))
}

fn auto_biber_download_url(asset: AutoBiberAsset) -> String {
    format!(
        "{SOURCEFORGE_BIBER_BASE_URL}/biblatex-biber/{}/binaries/{}/{}{}",
        asset.biber_version, asset.sourceforge_platform_dir, asset.archive_name, "/download"
    )
}

fn discover_biber_bin_dirs_from_env() -> Vec<PathBuf> {
    let Some(raw_dirs) = std::env::var_os(ENV_PDF_BIBER_BIN_DIRS) else {
        return Vec::new();
    };

    std::env::split_paths(&raw_dirs)
        .filter(|dir| resolve_biber_binary_in_dir(dir).is_some())
        .collect()
}

fn discover_biber_bin_dirs_from_path() -> Vec<PathBuf> {
    let Some(path_value) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    std::env::split_paths(&path_value)
        .filter(|dir| resolve_biber_binary_in_dir(dir).is_some())
        .collect()
}

fn resolve_biber_binary_in_dir(bin_dir: &Path) -> Option<PathBuf> {
    BIBER_BINARY_NAMES
        .iter()
        .map(|name| bin_dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn apply_biber_bin_dir_override(
    biber_bin_dir_override: Option<&Path>,
) -> Result<Option<PathEnvGuard>> {
    let Some(biber_bin_dir) = biber_bin_dir_override else {
        return Ok(None);
    };

    if !biber_bin_dir.is_dir() {
        return Err(anyhow!(
            "configured biber bin directory '{}' is not a directory",
            biber_bin_dir.display()
        ));
    }

    if resolve_biber_binary_in_dir(biber_bin_dir).is_none() {
        let supported_names = BIBER_BINARY_NAMES.join(" or ");
        return Err(anyhow!(
            "configured biber bin directory '{}' does not contain {}; provide a directory with a compatible biber binary",
            biber_bin_dir.display(),
            supported_names,
        ));
    }

    let original_path = std::env::var_os("PATH");
    let mut new_paths = vec![biber_bin_dir.to_path_buf()];
    if let Some(current_path) = original_path.as_ref() {
        new_paths.extend(std::env::split_paths(current_path));
    }
    let joined = std::env::join_paths(new_paths)
        .map_err(|error| anyhow!("failed to compose PATH with biber override: {error}"))?;

    // SAFETY: ferritex runs the build pipeline in a single-process CLI flow;
    // PATH is restored immediately after the PDF session.
    unsafe { std::env::set_var("PATH", &joined) };

    Ok(Some(PathEnvGuard { original_path }))
}

fn resolve_biber_bin_dir(cli_override: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = cli_override {
        return Some(path.to_path_buf());
    }

    std::env::var_os(ENV_PDF_BIBER_BIN_DIR).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}

fn first_non_empty_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("<no error line>")
        .trim()
        .to_string()
}

fn build_pdf_primary_input(input_name: &str) -> String {
    let mut source = String::with_capacity(PDF_COMPAT_SHIMS.len() + input_name.len() + 16);
    source.push_str(PDF_COMPAT_SHIMS);
    source.push_str("\\input{");
    source.push_str(input_name);
    source.push_str("}\n");
    source
}

fn read_text_if_exists(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn probe_log_excerpt(log_text: &str) -> String {
    let lines = log_text.lines().collect::<Vec<_>>();
    let mut selected = Vec::new();
    for (idx, raw_line) in lines.iter().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('!')
            || line.contains("Error")
            || line.contains("Undefined control sequence")
            || line.contains("Emergency stop")
        {
            selected.push(line.to_string());
            if let Some(next_line) = lines.get(idx + 1) {
                let next = next_line.trim();
                if next.starts_with("l.") {
                    selected.push(next.to_string());
                }
            }
            if let Some(next_line) = lines.get(idx + 2) {
                let next = next_line.trim();
                if next.starts_with("l.") {
                    selected.push(next.to_string());
                }
            }
            if selected.len() == 3 {
                break;
            }
        }
    }

    if selected.is_empty() {
        selected.extend(
            log_text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(3)
                .map(ToOwned::to_owned),
        );
    }

    selected.join(" | ")
}

fn external_tool_excerpt(log_text: &str) -> String {
    let lines = log_text.lines().collect::<Vec<_>>();
    let mut selected = Vec::new();
    for line in lines.iter().map(|entry| entry.trim()) {
        if line.is_empty() {
            continue;
        }
        if line.contains("ERROR")
            || line.contains("Error")
            || line.contains("FATAL")
            || line.contains("cannot")
            || line.contains("failed")
        {
            selected.push(line.to_string());
            if selected.len() == 4 {
                break;
            }
        }
    }

    if selected.is_empty() {
        selected.extend(
            lines
                .iter()
                .map(|entry| entry.trim())
                .filter(|line| !line.is_empty())
                .take(4)
                .map(ToOwned::to_owned),
        );
    }

    selected.join(" | ")
}

fn parse_version_after_marker(haystack: &str, marker: &str) -> Option<String> {
    let start = haystack.find(marker)? + marker.len();
    let tail = haystack.get(start..)?;
    let mut version = String::new();
    for ch in tail.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            version.push(ch);
        } else {
            break;
        }
    }

    if version.is_empty() {
        None
    } else {
        Some(version.trim_end_matches('.').to_string())
    }
}

fn parse_biber_mismatch_versions(text: &str) -> Option<(String, String)> {
    if !text.contains("Found biblatex control file version ") {
        return None;
    }

    let observed = parse_version_after_marker(text, "Found biblatex control file version ")
        .unwrap_or_else(|| "unknown".to_string());
    let expected = parse_version_after_marker(text, "expected version ")
        .unwrap_or_else(|| "unknown".to_string());
    Some((observed, expected))
}

fn biber_bcf_version_mismatch_hint(
    external_tool_excerpt: &Option<String>,
    error_logs: &[String],
) -> Option<String> {
    let mut sources = Vec::new();
    if let Some(excerpt) = external_tool_excerpt {
        sources.push(excerpt.as_str());
    }
    for entry in error_logs {
        sources.push(entry.as_str());
    }

    for source in sources {
        if let Some((observed, expected)) = parse_biber_mismatch_versions(source) {
            let matrix_guidance = auto_biber_matrix_guidance(&observed);
            return Some(format!(
                "biber/biblatex compatibility mismatch detected (BCF {observed}, biber expects {expected}); install a matching biber for the active TeX bundle or pass --pdf-biber-bin-dir <DIR> (or env {ENV_PDF_BIBER_BIN_DIR}) that contains a compatible 'biber'. Auto mode retries PATH candidates and can auto-install a compatible biber when --tool-install-policy allows it (and {ENV_PDF_BIBER_AUTO_INSTALL} is not disabled); you can also provide additional candidate directories via {ENV_PDF_BIBER_BIN_DIRS}. {matrix_guidance}"
            ));
        }
    }

    None
}

fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(current) = source {
        chain.push(current.to_string());
        source = current.source();
    }
    chain.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::{
        AutoInstallPermission, ToolInstallPolicy, auto_biber_matrix_guidance,
        biber_bcf_version_mismatch_hint, evaluate_biber_auto_install_policy, external_tool_excerpt,
        interactive_prompt_supported, parse_bcf_controlfile_version, parse_biber_mismatch_versions,
        resolve_auto_biber_asset_for_bcf,
    };

    #[test]
    fn detects_biber_bcf_version_mismatch_from_excerpt() {
        let excerpt = Some(
            "ERROR - Error: Found biblatex control file version 3.8, expected version 3.11."
                .to_string(),
        );
        let hint = biber_bcf_version_mismatch_hint(&excerpt, &[]);

        assert!(hint.is_some());
        let hint = hint.unwrap_or_default();
        assert!(hint.contains("BCF 3.8"));
        assert!(hint.contains("3.11"));
        assert!(hint.contains("--pdf-biber-bin-dir"));
        assert!(hint.contains("FERRITEX_PDF_BIBER_BIN_DIRS"));
        assert!(hint.contains("built-in auto-install"));
    }

    #[test]
    fn no_biber_hint_for_unrelated_error_logs() {
        let excerpt = Some("ERROR - generic external tool failure".to_string());
        let hint = biber_bcf_version_mismatch_hint(&excerpt, &[]);

        assert!(hint.is_none());
    }

    #[test]
    fn external_tool_excerpt_prefers_error_lines() {
        let text = "INFO - preface\nERROR - failed to process bibliography\nINFO - ERRORS: 1\n";
        let excerpt = external_tool_excerpt(text);

        assert!(excerpt.contains("ERROR - failed to process bibliography"));
    }

    #[test]
    fn parses_biber_mismatch_versions_from_error_text() {
        let parsed = parse_biber_mismatch_versions(
            "ERROR - Error: Found biblatex control file version 3.8, expected version 3.11.",
        );
        assert_eq!(parsed, Some(("3.8".to_string(), "3.11".to_string())));
    }

    #[test]
    fn autoinstall_asset_mapping_for_bcf_3_8_is_platform_specific() {
        let asset = resolve_auto_biber_asset_for_bcf("3.8");

        if cfg!(any(
            all(
                target_os = "macos",
                any(target_arch = "aarch64", target_arch = "x86_64")
            ),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64")
        )) {
            assert!(asset.is_some());
        } else {
            assert!(asset.is_none());
        }
    }

    #[test]
    fn parse_bcf_controlfile_version_reads_controlfile_attribute() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<bcf:controlfile version="3.8" bltxversion="3.17" xmlns:bcf="https://sourceforge.net/projects/biblatex">
</bcf:controlfile>"#;
        assert_eq!(parse_bcf_controlfile_version(text), Some("3.8".to_string()));
    }

    #[test]
    fn unsupported_bcf_matrix_guidance_lists_supported_scope() {
        let guidance = auto_biber_matrix_guidance("9.9");
        assert!(guidance.contains("supports only"));
        assert!(guidance.contains("BCF 3.8"));
        assert!(guidance.contains("biber 2.17"));
        assert!(guidance.contains("9.9"));
    }

    #[test]
    fn never_policy_disables_auto_install_with_guidance() {
        let decision = evaluate_biber_auto_install_policy(ToolInstallPolicy::Never, "3.8", "3.11")
            .expect("policy evaluation should not fail");
        match decision {
            AutoInstallPermission::Allowed => {
                panic!("never policy must not allow automatic installs")
            }
            AutoInstallPermission::Denied(reason) => {
                assert!(reason.contains("--tool-install-policy never"));
                assert!(reason.contains("rerun ferritex"));
            }
        }
    }

    #[test]
    fn ask_policy_noninteractive_returns_actionable_guidance() {
        if interactive_prompt_supported() {
            return;
        }

        let decision = evaluate_biber_auto_install_policy(ToolInstallPolicy::Ask, "3.8", "3.11")
            .expect("policy evaluation should not fail");
        match decision {
            AutoInstallPermission::Allowed => {
                panic!("ask policy should not auto-allow install in non-interactive mode")
            }
            AutoInstallPermission::Denied(reason) => {
                assert!(reason.contains("--tool-install-policy auto"));
                assert!(reason.contains("manually"));
            }
        }
    }
}
