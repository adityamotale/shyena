//! Invocation of user-defined PowerShell functions and error normalisation.
//!
//! Functions run on a worker thread; only failures cross back to the UI as a
//! single-line message.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fs::File;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ERROR_LINE_MAX_CHARS;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

/// Runs a PowerShell process with output redirected to temporary files.
///
/// `Command::output()` cannot be used here: a GUI child spawned by a function
/// (e.g. `Start-Process notepad`) inherits the pipe write handles, so waiting
/// for EOF would hang forever. A unique sequence number keeps concurrent
/// invocations from sharing those files.
fn run_powershell(program: &str, args: &[&str]) -> std::io::Result<Output> {
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let out_path = dir.join(format!("shyena-out-{pid}-{seq}.txt"));
    let err_path = dir.join(format!("shyena-err-{pid}-{seq}.txt"));

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(File::create(&out_path)?);
    cmd.stderr(File::create(&err_path)?);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let status = cmd.status()?;

    let stdout = std::fs::read(&out_path).unwrap_or_default();
    let stderr = std::fs::read(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Functions that index `$args` cannot run meaningfully without at least one
/// dynamic token; without this guard PowerShell coerces `$args[0]` to `$null`
/// (and `[int]$null` to `0`), silently applying the wrong value.
fn indexes_dynamic_args(func_body: &str) -> bool {
    func_body.to_ascii_lowercase().contains("$args[")
}

/// Windows PowerShell serializes redirected stderr as CLIXML; pull the first
/// `<S S="Error">` record out and decode its `_xHHHH_` escapes.
fn decode_clixml_error(raw: &str) -> Option<String> {
    const OPEN: &str = "<S S=\"Error\">";
    const CLOSE: &str = "</S>";
    /// `&quot;` is the longest entity emitted here, so at most five characters
    /// follow the `&` before the terminating `;`.
    const MAX_ENTITY_TAIL: usize = 5;

    let start = raw.find(OPEN)? + OPEN.len();
    let end = start + raw[start..].find(CLOSE)?;
    let encoded = &raw[start..end];

    let mut out = String::with_capacity(encoded.len());
    let mut chars = encoded.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '_' || chars.peek() != Some(&'x') {
            match ch {
                '&' => {
                    let mut entity = String::from("&");
                    for _ in 0..MAX_ENTITY_TAIL {
                        if let Some(next) = chars.next() {
                            entity.push(next);
                            if next == ';' {
                                break;
                            }
                        }
                    }
                    match entity.as_str() {
                        "&lt;" => out.push('<'),
                        "&gt;" => out.push('>'),
                        "&amp;" => out.push('&'),
                        "&quot;" => out.push('"'),
                        "&apos;" => out.push('\''),
                        _ => out.push_str(&entity),
                    }
                }
                _ => out.push(ch),
            }
            continue;
        }

        chars.next(); // 'x'
        let hex: String = chars.by_ref().take(4).collect();
        let code = u32::from_str_radix(&hex, 16).ok();
        if hex.len() == 4 && chars.peek() == Some(&'_') && code.is_some() {
            chars.next(); // closing '_'
            if let Some(decoded) = code.and_then(char::from_u32) {
                out.push(decoded);
            }
        } else {
            // Not an escape after all; preserve the original bytes.
            out.push_str("_x");
            out.push_str(&hex);
        }
    }

    Some(out)
}

/// Collapses a verbose PowerShell error into a single line suitable for the
/// in-app error overlay, prefixed with the command that failed.
pub fn format_ps_error(func_name: &str, error: &str) -> String {
    let raw = error
        .strip_prefix("PowerShell Execution Error: ")
        .unwrap_or(error);
    let decoded = decode_clixml_error(raw).unwrap_or_else(|| raw.to_string());

    let first_line = decoded
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("execution failed");

    let detail = first_line
        .strip_prefix(func_name)
        .and_then(|rest| rest.strip_prefix(" : "))
        .unwrap_or(first_line);
    let collapsed = detail.split_whitespace().collect::<Vec<_>>().join(" ");

    let condensed: String = if collapsed.chars().count() > ERROR_LINE_MAX_CHARS {
        format!(
            "{}...",
            collapsed
                .chars()
                .take(ERROR_LINE_MAX_CHARS)
                .collect::<String>()
        )
    } else {
        collapsed
    };

    if condensed.starts_with(func_name) && condensed.as_bytes().get(func_name.len()) == Some(&b':')
    {
        condensed
    } else {
        format!("{func_name}: {condensed}")
    }
}

/// Executes a parsed PowerShell function with dynamic arguments and captures output
pub fn dispatch_ps_function(
    func_name: &str,
    func_body: &str,
    args: &[&str],
) -> Result<String, String> {
    if args.is_empty() && indexes_dynamic_args(func_body) {
        return Err("requires at least one argument".to_string());
    }

    // Definition and invocation share one script block so helper variables do
    // not leak into the caller's scope. `ErrorActionPreference` escalates
    // non-terminating failures (e.g. a failed WMI call) into real errors.
    let safe_body = if func_body.trim().starts_with('{') {
        func_body.to_string()
    } else {
        format!("{{\n{func_body}\n}}")
    };

    let args_joined = args.join(" ");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; & {{ function {func_name} {safe_body} ; {func_name} {args_joined} }}"
    );

    // `-EncodedCommand` accepts UTF-16LE only, regardless of host locale.
    let utf16_bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded_cmd = STANDARD.encode(&utf16_bytes);

    // Bypass avoids policy-driven silent failures on locked-down machines;
    // the other flags keep the process non-interactive and invisible.
    let ps_args = [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-EncodedCommand",
        &encoded_cmd,
    ];

    // Prefer Windows PowerShell 5.1: the bundled functions use WMI APIs such
    // as Get-WmiObject that are absent from PowerShell 7. Fall back to `pwsh`
    // only when 5.1 is missing or fails to launch the script.
    let output = match run_powershell("powershell", &ps_args) {
        Ok(out) if out.status.success() => out,
        Ok(primary) => match run_powershell("pwsh", &ps_args) {
            Ok(fallback) if fallback.status.success() => fallback,
            Ok(_) | Err(_) => primary,
        },
        Err(primary_err) => match run_powershell("pwsh", &ps_args) {
            Ok(out) => out,
            Err(fallback_err) => {
                return Err(format!(
                    "Failed to spawn PowerShell (powershell: {primary_err}; pwsh: {fallback_err})"
                ));
            }
        },
    };

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("PowerShell Execution Error: {}", err_msg.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_args_rejected_for_functions_indexing_args() {
        let body = "{ $target = [int]$args[0]; $target }";
        let err = dispatch_ps_function("brightness", body, &[]).unwrap_err();
        assert_eq!(err, "requires at least one argument");
        assert!(!indexes_dynamic_args("{ Write-Output $args }"));
    }

    #[test]
    fn test_format_ps_error_condenses_verbose_output() {
        let raw = "PowerShell Execution Error: brightness : Cannot process command because of \
                   one or more missing mandatory parameters: Level.\nAt line:1 char:77\n\
                   + CategoryInfo : InvalidArgument\n";
        assert_eq!(
            format_ps_error("brightness", raw),
            "brightness: Cannot process command because of one or more missing mandatory parameters: Level."
        );
    }

    #[test]
    fn test_format_ps_error_decodes_clixml() {
        let raw = "PowerShell Execution Error: #< CLIXML\n<Objs Version=\"1.1.0.1\"><S S=\"Error\">\
                   brightness : Cannot process command because of one or more missing mandatory \
                   parameters: Level._x000D__x000A_</S><S S=\"Error\">At line:1 char:77</S></Objs>";
        assert_eq!(
            format_ps_error("brightness", raw),
            "brightness: Cannot process command because of one or more missing mandatory parameters: Level."
        );
    }

    #[test]
    fn test_format_ps_error_truncates_long_errors() {
        let raw = "x".repeat(200);
        let formatted = format_ps_error("cmd", &raw);
        assert_eq!(formatted.chars().count(), 5 + ERROR_LINE_MAX_CHARS + 3);
        assert!(formatted.ends_with("..."));
    }

    #[test]
    fn test_format_ps_error_keeps_short_errors() {
        assert_eq!(
            format_ps_error("echo", "already prefixed: bad input"),
            "echo: already prefixed: bad input"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_dispatch_ps_function_returns_stdout() {
        let out = dispatch_ps_function("shyena-echo", "{ Write-Output 'ok' }", &[]).unwrap();
        assert_eq!(out, "ok");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_mandatory_parameter_requires_argument() {
        let body = "{ param([Parameter(Mandatory = $true)][int]$Level) Write-Output $Level }";

        let err = dispatch_ps_function("brightness", body, &[]).unwrap_err();
        assert!(err.contains("MissingMandatoryParameter"), "{}", err);
        assert_eq!(
            format_ps_error("brightness", &err),
            "brightness: Cannot process command because of one or more missing mandatory parameters: Level.",
            "raw: {err}"
        );

        let out = dispatch_ps_function("brightness", body, &["42"]).unwrap();
        assert_eq!(out, "42");
    }
}
