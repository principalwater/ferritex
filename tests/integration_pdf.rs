#[path = "common/base.rs"]
mod common;

use ferritex::build::{BuildConfig, OutputFormat, PdfBiberMode, ToolInstallPolicy, run_build};
#[cfg(unix)]
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsString,
    sync::{Mutex, OnceLock},
};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct PathEnvGuard {
    original_path: Option<OsString>,
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        if let Some(path) = self.original_path.as_ref() {
            // SAFETY: tests serialize PATH mutation using ENV_LOCK.
            unsafe { std::env::set_var("PATH", path) };
        } else {
            // SAFETY: tests serialize PATH mutation using ENV_LOCK.
            unsafe { std::env::remove_var("PATH") };
        }
    }
}

struct EnvVarGuard {
    key: &'static str,
    original_value: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original_value = std::env::var_os(key);
        // SAFETY: tests serialize env mutation using ENV_LOCK.
        unsafe { std::env::set_var(key, value) };
        Self {
            key,
            original_value,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original_value.as_ref() {
            // SAFETY: tests serialize env mutation using ENV_LOCK.
            unsafe { std::env::set_var(self.key, value) };
        } else {
            // SAFETY: tests serialize env mutation using ENV_LOCK.
            unsafe { std::env::remove_var(self.key) };
        }
    }
}

#[cfg(unix)]
fn write_bad_biber_script(path: &std::path::Path) {
    std::fs::write(
        path,
        "#!/bin/sh\n\
echo \"ERROR - Error: Found biblatex control file version 3.8, expected version 3.11.\" 1>&2\n\
echo \"INFO - ERRORS: 1\" 1>&2\n\
exit 2\n",
    )
    .expect("failed to write bad biber script");

    let mut perms = std::fs::metadata(path)
        .expect("failed to stat bad biber script")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .expect("failed to set executable permission on bad biber script");
}

#[test]
fn pdf_build_path_is_wired_to_pdf_backend() {
    let input = common::fixture_path("simple.tex");
    let output_dir = std::env::temp_dir().join(format!("ferritex_pdf_test_{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Ask,
    );
    let result = run_build(&config).expect("PDF backend should compile via tectonic::latex_to_pdf");
    let pdf_path = result
        .pdf
        .expect("expected PDF artifact path in build result");
    let pdf_bytes = std::fs::read(&pdf_path).expect("failed to read generated PDF artifact");
    assert!(
        !pdf_bytes.is_empty(),
        "generated PDF artifact must not be empty"
    );
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "generated artifact is not a PDF file"
    );

    let _ = std::fs::remove_file(pdf_path);
    let _ = std::fs::remove_dir_all(output_dir);
}

#[cfg(unix)]
#[test]
fn pdf_fail_fast_guidance_mentions_biber_override_controls() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("failed to acquire env lock");

    let input = common::fixture_path("with_bibliography.tex");
    let output_dir = std::env::temp_dir().join(format!(
        "ferritex_pdf_biber_failfast_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let fake_biber_dir = output_dir.join("fake-biber-bin");
    std::fs::create_dir_all(&fake_biber_dir).expect("failed to create fake biber dir");
    let fake_biber_path = fake_biber_dir.join("biber");

    std::fs::write(
        &fake_biber_path,
        "#!/bin/sh\n\
echo \"ERROR - Error: Found biblatex control file version 3.8, expected version 3.11.\" 1>&2\n\
echo \"INFO - ERRORS: 1\" 1>&2\n\
exit 2\n",
    )
    .expect("failed to write fake biber script");

    let mut perms = std::fs::metadata(&fake_biber_path)
        .expect("failed to stat fake biber script")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_biber_path, perms)
        .expect("failed to set executable permission on fake biber script");

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        Some(&fake_biber_dir),
        PdfBiberMode::Strict,
        ToolInstallPolicy::Ask,
    );
    let error = run_build(&config).expect_err("PDF build should fail-fast with fake biber");
    let rendered = format!("{error:#}");

    assert!(
        rendered.contains("biber/biblatex compatibility mismatch detected"),
        "expected biber mismatch guidance, got: {rendered}"
    );
    assert!(
        rendered.contains("--pdf-biber-bin-dir"),
        "expected CLI override guidance, got: {rendered}"
    );
    assert!(
        rendered.contains("FERRITEX_PDF_BIBER_BIN_DIR"),
        "expected env override guidance, got: {rendered}"
    );
    assert!(
        rendered.contains("FERRITEX_PDF_BIBER_BIN_DIRS"),
        "expected auto-candidates env guidance, got: {rendered}"
    );

    let _ = std::fs::remove_dir_all(output_dir);
}

#[cfg(unix)]
#[test]
fn pdf_auto_biber_mode_retries_with_alternate_candidate() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("failed to acquire env lock");

    let input = common::fixture_path("with_bibliography.tex");
    let output_dir = std::env::temp_dir().join(format!(
        "ferritex_pdf_biber_auto_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let bad_biber_dir = output_dir.join("bad-biber-bin");
    std::fs::create_dir_all(&bad_biber_dir).expect("failed to create bad biber dir");
    let bad_biber_path = bad_biber_dir.join("biber");
    std::fs::write(
        &bad_biber_path,
        "#!/bin/sh\n\
echo \"ERROR - Error: Found biblatex control file version 3.8, expected version 3.11.\" 1>&2\n\
echo \"INFO - ERRORS: 1\" 1>&2\n\
exit 2\n",
    )
    .expect("failed to write bad biber script");

    let good_biber_dir = output_dir.join("good-biber-bin");
    std::fs::create_dir_all(&good_biber_dir).expect("failed to create good biber dir");
    let good_biber_path = good_biber_dir.join("biber");
    std::fs::write(
        &good_biber_path,
        "#!/bin/sh\n\
name=\"\"\n\
for arg in \"$@\"; do\n\
  case \"$arg\" in\n\
    -*) ;;\n\
    *) name=\"$arg\"; break ;;\n\
  esac\n\
done\n\
if [ -z \"$name\" ]; then\n\
  name=\"with_bibliography\"\n\
fi\n\
cat > \"${name}.bbl\" <<'EOF'\n\
% $ biblatex auxiliary file $\n\
\\begingroup\n\
\\endgroup\n\
\\endinput\n\
EOF\n\
exit 0\n",
    )
    .expect("failed to write good biber script");

    for path in [&bad_biber_path, &good_biber_path] {
        let mut perms = std::fs::metadata(path)
            .expect("failed to stat fake biber script")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)
            .expect("failed to set executable permission on fake biber script");
    }

    let original_path = std::env::var_os("PATH");
    let _path_guard = PathEnvGuard {
        original_path: original_path.clone(),
    };
    let mut updated_paths = vec![bad_biber_dir.clone(), good_biber_dir.clone()];
    if let Some(current) = original_path.as_ref() {
        updated_paths.extend(std::env::split_paths(current));
    }
    let joined = std::env::join_paths(updated_paths).expect("failed to compose PATH");
    // SAFETY: tests serialize PATH mutation using ENV_LOCK.
    unsafe { std::env::set_var("PATH", joined) };

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Ask,
    );
    let result = run_build(&config).expect("PDF build should recover with second biber candidate");
    let pdf_path = result
        .pdf
        .expect("expected PDF artifact path in build result");
    let pdf_bytes = std::fs::read(&pdf_path).expect("failed to read generated PDF artifact");
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "generated artifact is not a PDF file"
    );

    let _ = std::fs::remove_dir_all(output_dir);
}

#[cfg(unix)]
#[test]
fn pdf_tool_install_policy_never_fails_with_restart_guidance() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("failed to acquire env lock");

    let input = common::fixture_path("with_bibliography.tex");
    let output_dir = std::env::temp_dir().join(format!(
        "ferritex_pdf_tool_policy_never_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let bad_biber_dir = output_dir.join("bad-biber-bin");
    std::fs::create_dir_all(&bad_biber_dir).expect("failed to create bad biber dir");
    write_bad_biber_script(&bad_biber_dir.join("biber"));

    let original_path = std::env::var_os("PATH");
    let _path_guard = PathEnvGuard {
        original_path: original_path.clone(),
    };
    // SAFETY: tests serialize PATH mutation using ENV_LOCK.
    unsafe { std::env::set_var("PATH", &bad_biber_dir) };

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Never,
    );
    let error = run_build(&config).expect_err("PDF build should fail with never policy");
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains("--tool-install-policy never"),
        "expected never-policy guidance, got: {rendered}"
    );
    assert!(
        rendered.contains("rerun ferritex"),
        "expected rerun guidance, got: {rendered}"
    );

    let _ = std::fs::remove_dir_all(output_dir);
}

#[cfg(unix)]
#[test]
fn pdf_tool_install_policy_ask_noninteractive_fails_with_auto_hint() {
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return;
    }

    let _env_lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("failed to acquire env lock");

    let input = common::fixture_path("with_bibliography.tex");
    let output_dir = std::env::temp_dir().join(format!(
        "ferritex_pdf_tool_policy_ask_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let bad_biber_dir = output_dir.join("bad-biber-bin");
    std::fs::create_dir_all(&bad_biber_dir).expect("failed to create bad biber dir");
    write_bad_biber_script(&bad_biber_dir.join("biber"));

    let original_path = std::env::var_os("PATH");
    let _path_guard = PathEnvGuard {
        original_path: original_path.clone(),
    };
    // SAFETY: tests serialize PATH mutation using ENV_LOCK.
    unsafe { std::env::set_var("PATH", &bad_biber_dir) };

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Ask,
    );
    let error = run_build(&config).expect_err("PDF build should fail in non-interactive ask mode");
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains("non-interactive"),
        "expected non-interactive guidance, got: {rendered}"
    );
    assert!(
        rendered.contains("--tool-install-policy auto"),
        "expected auto-policy guidance, got: {rendered}"
    );

    let _ = std::fs::remove_dir_all(output_dir);
}

#[test]
fn pdf_respects_source_date_epoch_for_tex_year_primitive() {
    let _env_lock = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("failed to acquire env lock");
    let _source_date_guard = EnvVarGuard::set("SOURCE_DATE_EPOCH", "1893456000");

    let input = common::fixture_path("year_probe.tex");
    let output_dir = std::env::temp_dir().join(format!(
        "ferritex_pdf_year_probe_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Pdf,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Ask,
    );
    run_build(&config).expect("PDF build should succeed for year probe fixture");

    let log_path = output_dir.join("year_probe.log");
    let log_text = std::fs::read_to_string(&log_path).expect("failed to read generated TeX log");
    assert!(
        log_text.contains("FERRITEX_YEAR=2030"),
        "expected TeX year to follow SOURCE_DATE_EPOCH, got log excerpt: {}",
        log_text.lines().take(40).collect::<Vec<_>>().join(" | ")
    );

    let _ = std::fs::remove_dir_all(output_dir);
}
