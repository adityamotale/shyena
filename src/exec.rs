use std::process::Command;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Executes a parsed PowerShell function with dynamic arguments and captures output
pub fn dispatch_ps_function(
    func_name: &str,
    func_body: &str,
    args: &[&str],
) -> Result<String, String> {
    // 1. Wrap in an execution block to ensure proper scope and evaluation
    let args_joined = args.join(" ");
    let script = format!(
        "& {{ function {} {} ; {} {} }}",
        func_name, func_body, func_name, args_joined
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

    // 4. Attempt pwsh (PowerShell 7+), fallback to powershell (Windows PowerShell 5.1)
    let mut cmd = Command::new("pwsh");
    cmd.args(&ps_args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().or_else(|_| {
        let mut fb = Command::new("powershell");
        fb.args(&ps_args);
        #[cfg(target_os = "windows")]
        fb.creation_flags(CREATE_NO_WINDOW);
        fb.output()
    }).map_err(|e| format!("Failed to spawn PowerShell executable: {}", e))?;

    // 5. Handle execution results
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("PowerShell Execution Error: {}", err_msg.trim()))
    }
}
