use std::fs::File;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
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

/// Executes a parsed PowerShell function with dynamic arguments and captures output
pub fn dispatch_ps_function(
    func_name: &str,
    func_body: &str,
    args: &[&str],
) -> Result<String, String> {
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
