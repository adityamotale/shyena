use std::fs::File;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_ERROR_CHARS: usize = 120;
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

fn run_powershell(program: &str, args: &[&str]) -> std::io::Result<Output> {
    // Capture to files instead of `output()` pipes: a GUI child spawned by a
    // PowerShell function (e.g. Start-Process notepad) can inherit the pipe
    // write handles and keep `output()` waiting forever for EOF.
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let out_path = dir.join(format!("shyena-out-{}-{}.txt", pid, seq));
    let err_path = dir.join(format!("shyena-err-{}-{}.txt", pid, seq));

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(File::create(&out_path)?);
    cmd.stderr(File::create(&err_path)?);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let status = cmd.status()?;
    drop(cmd);

    let stdout = std::fs::read(&out_path).unwrap_or_default();
    let stderr = std::fs::read(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);

    Ok(Output { status, stdout, stderr })
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
                    for _ in 0..5 {
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

        chars.next(); // consume 'x'
        let hex: String = chars.by_ref().take(4).collect();
        if hex.len() == 4 && chars.peek() == Some(&'_') && u32::from_str_radix(&hex, 16).is_ok() {
            chars.next(); // consume closing '_'
            if let Some(decoded) = char::from_u32(u32::from_str_radix(&hex, 16).unwrap()) {
                out.push(decoded);
            }
        } else {
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

    let prefix = format!("{} : ", func_name);
    let detail = first_line.strip_prefix(&prefix).unwrap_or(first_line);
    let collapsed = detail.split_whitespace().collect::<Vec<_>>().join(" ");

    let condensed: String = if collapsed.chars().count() > MAX_ERROR_CHARS {
        format!(
            "{}...",
            collapsed.chars().take(MAX_ERROR_CHARS).collect::<String>()
        )
    } else {
        collapsed
    };

    if condensed.starts_with(&format!("{}:", func_name)) {
        condensed
    } else {
        format!("{}: {}", func_name, condensed)
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

    // 1. Wrap in an execution block to ensure proper scope and evaluation.
    // $ErrorActionPreference = 'Stop' turns non-terminating errors (e.g. a failed
    // WMI method call) into real failures so they aren't silently ignored.
    let safe_body = if func_body.trim().starts_with('{') {
        func_body.to_string()
    } else {
        format!("{{\n{}\n}}", func_body)
    };

    let args_joined = args.join(" ");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; & {{ function {} {} ; {} {} }}",
        func_name, safe_body, func_name, args_joined
    );

    // 2. Encode strictly to UTF-16LE for Windows PowerShell compatibility
    let utf16_bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let encoded_cmd = STANDARD.encode(&utf16_bytes);

    // 3. Define execution arguments
    let ps_args = [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass", // Prevents silent failures on restricted systems
        "-WindowStyle",
        "Hidden",
        "-EncodedCommand",
        &encoded_cmd,
    ];

    // 4. Prefer Windows PowerShell 5.1: WMI functions like Get-WmiObject and
    // WmiSetBrightness are 5.1-only. Fall back to pwsh (7+) if 5.1 is missing
    // or fails to execute the script.
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
                    "Failed to spawn PowerShell (powershell: {}; pwsh: {})",
                    primary_err, fallback_err
                ));
            }
        },
    };

    // 5. Handle execution results
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
        assert_eq!(formatted.chars().count(), 5 + MAX_ERROR_CHARS + 3);
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
    fn test_mandatory_parameter_requires_argument() {
        let body = "{ param([Parameter(Mandatory = $true)][int]$Level) Write-Output $Level }";

        let err = dispatch_ps_function("brightness", body, &[]).unwrap_err();
        assert!(err.contains("MissingMandatoryParameter"), "{}", err);
        assert_eq!(
            format_ps_error("brightness", &err),
            "brightness: Cannot process command because of one or more missing mandatory parameters: Level.",
            "raw: {}",
            err
        );

        let out = dispatch_ps_function("brightness", body, &["42"]).unwrap();
        assert_eq!(out, "42");
    }
}
