#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use serde_json::json;
use serde::{Deserialize, Serialize};

// ------------------------
// ANSI cleaner
// ------------------------
fn clean_ansi(input: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(input);
    String::from_utf8_lossy(&stripped).to_string()
}

// -------------------------
// ✅ DB path (outside src-tauri)
// Windows: %LOCALAPPDATA%\personaliz-desktop\personaliz.sqlite
// -------------------------
fn db_file_path() -> std::path::PathBuf {
    use std::path::PathBuf;

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(base).join("personaliz-desktop");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("personaliz.sqlite");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(base).join(".personaliz-desktop");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("personaliz.sqlite");
    }
}

// -------------------------
// ✅ Ensure tables exist
// -------------------------
fn ensure_logs_table(conn: &rusqlite::Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS logs (
            id TEXT PRIMARY KEY,
            agent_id TEXT NULL,
            timestamp TEXT NOT NULL,
            level TEXT NOT NULL,
            message TEXT NOT NULL,
            llm_used TEXT NULL,
            status TEXT NULL,
            error TEXT NULL
        )",
        [],
    );
}

fn ensure_user_settings_table(conn: &rusqlite::Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS user_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            llm_api_key TEXT NULL,
            llm_provider TEXT NULL,
            updated_at TEXT NULL
        )",
        [],
    );

    // Ensure the single row exists
    let _ = conn.execute(
        "INSERT OR IGNORE INTO user_settings (id, llm_api_key, llm_provider, updated_at)
         VALUES (1, NULL, NULL, NULL)",
        [],
    );
}

fn open_db() -> Result<rusqlite::Connection, String> {
    use rusqlite::Connection;
    let path = db_file_path();
    Connection::open(path).map_err(|e| format!("❌ Could not open DB: {}", e))
}

// -------------------------
// ✅ SQLite logger
// -------------------------
fn write_log(level: &str, message: &str) {
    use rusqlite::{params, Connection};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    let path = db_file_path();
    let conn = match Connection::open(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    ensure_logs_table(&conn);
    ensure_user_settings_table(&conn);

    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return,
    };

    let id = Uuid::new_v4().to_string();

    let _ = conn.execute(
        "INSERT INTO logs (id, agent_id, timestamp, level, message, llm_used, status, error)
         VALUES (?1, NULL, ?2, ?3, ?4, NULL, NULL, NULL)",
        params![id, now.to_string(), level, message],
    );
}

// -------------------------
// ✅ Read last N logs
// -------------------------
fn read_last_logs(limit: i64) -> String {
    use rusqlite::Connection;

    let path = db_file_path();
    let conn = match Connection::open(path) {
        Ok(c) => c,
        Err(e) => return format!("❌ Could not open DB: {}", e),
    };

    ensure_logs_table(&conn);
    ensure_user_settings_table(&conn);

    let mut stmt = match conn.prepare(
        "SELECT timestamp, level, message
         FROM logs
         ORDER BY timestamp DESC
         LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(e) => return format!("❌ Failed to prepare logs query: {}", e),
    };

    let rows = stmt.query_map([limit], |row| {
        let ts: String = row.get(0)?;
        let level: String = row.get(1)?;
        let msg: String = row.get(2)?;
        Ok((ts, level, msg))
    });

    match rows {
        Ok(iter) => {
            let mut items: Vec<(String, String, String)> = vec![];
            for r in iter.flatten() {
                items.push(r);
            }

            if items.is_empty() {
                return "ℹ️ No logs yet.".to_string();
            }

            let mut out = String::from("🧾 Last logs:\n\n");
            for (ts, level, msg) in items {
                out.push_str(&format!("[{}] {} — {}\n", ts, level, msg));
            }
            out
        }
        Err(e) => format!("❌ Failed reading logs: {}", e),
    }
}

// -------------------------
// ✅ User settings helpers
// KEY ONLY matters; provider ignored.
// -------------------------
fn get_saved_api_key() -> Option<String> {
    use rusqlite::OptionalExtension;

    let conn = open_db().ok()?;
    ensure_user_settings_table(&conn);

    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT llm_api_key FROM user_settings WHERE id=1",
            [],
            |r| Ok(r.get(0)?),
        )
        .optional()
        .ok()?;

    match row {
        Some(Some(k)) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

#[tauri::command]
fn save_user_api_key(llm_api_key: String, llm_provider: String) -> Result<String, String> {
    // provider is ignored, but we store "gemini" for display clarity
    use rusqlite::params;

    let conn = open_db()?;
    ensure_user_settings_table(&conn);

    conn.execute(
        "UPDATE user_settings
         SET llm_api_key = ?1, llm_provider = ?2, updated_at = datetime('now')
         WHERE id=1",
        params![llm_api_key, "gemini"],
    )
    .map_err(|e| format!("DB update failed: {}", e))?;

    write_log("INFO", &format!("Saved Gemini API key (provider arg was: {})", llm_provider));
    Ok("✅ Saved Gemini API key.".to_string())
}

#[tauri::command]
fn get_user_settings() -> Result<String, String> {
    let conn = open_db()?;
    ensure_user_settings_table(&conn);

    let (key, provider): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT llm_api_key, llm_provider FROM user_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("DB read failed: {}", e))?;

    Ok(format!(
        "llm_api_key: {}\nllm_provider: {}",
        if key.is_some() { "✅ set" } else { "❌ not set" },
        provider.unwrap_or_else(|| "gemini".to_string())
    ))
}

// ------------------------
// 1) Safe command executor
// ------------------------
#[tauri::command]
fn send_message(message: String) -> String {
    let trimmed = message.trim();
    let msg = trimmed.to_lowercase();

    write_log("INFO", &format!("User ran command: {}", trimmed));

    // ✅ Special command: show logs
    if msg == "show logs" || msg == "logs" || msg == "openclaw logs" {
        write_log("INFO", "User requested logs");
        return read_last_logs(10);
    }

    // ✅ Allowed command prefixes (safe list)
    let allowed = ["whoami", "dir", "echo", "openclaw", "node", "npm", "python", "setup openclaw"];

    let mut is_allowed = false;
    for cmd in allowed.iter() {
        if msg.starts_with(cmd) {
            is_allowed = true;
            break;
        }
    }

    if !is_allowed {
        write_log("WARN", &format!("Blocked command: {}", trimmed));
        return "Blocked ❌: This command is not allowed for safety.\nTry: whoami, dir, echo, node -v, openclaw, or setup openclaw"
            .to_string();
    }

    // If user types "setup openclaw" in chat, call function directly
    if msg == "setup openclaw" {
        return setup_openclaw();
    }

    let output = Command::new("cmd").args(["/C", trimmed]).output();

    match output {
        Ok(result) => {
            if result.stdout.is_empty() {
                let err = clean_ansi(&result.stderr);
                if err.trim().is_empty() {
                    write_log("INFO", "Command output: (No output)");
                    "(No output)".to_string()
                } else {
                    write_log("ERROR", &format!("Command error output: {}", err));
                    err
                }
            } else {
                let out = clean_ansi(&result.stdout);
                write_log("INFO", &format!("Command output: {}", out));
                out
            }
        }
        Err(e) => {
            let err = format!("Failed to execute command: {}", e);
            write_log("ERROR", &err);
            err
        }
    }
}

// -----------------------------------------
// 2) OpenClaw setup
// -----------------------------------------
#[tauri::command]
fn setup_openclaw() -> String {
    write_log("INFO", "Setup OpenClaw triggered");

    let mut log: Vec<String> = vec![];

    log.push("🔎 Checking Node.js...".to_string());
    let node_check = Command::new("cmd").args(["/C", "node -v"]).output();

    if node_check.is_err() || node_check.as_ref().unwrap().status.success() == false {
        log.push("❌ Node.js not found. Please install Node.js first, then retry Setup OpenClaw.".to_string());
        write_log("ERROR", "Setup failed: Node.js not found");
        return log.join("\n");
    } else {
        let v = clean_ansi(&node_check.unwrap().stdout);
        log.push(format!("✅ Node found: {}", v.trim()));
    }

    log.push("🔎 Checking if OpenClaw is installed...".to_string());
    let check = Command::new("cmd").args(["/C", "where openclaw"]).output();

    let is_installed = match check {
        Ok(res) => res.status.success() && !res.stdout.is_empty(),
        Err(_) => false,
    };

    if !is_installed {
        log.push("⬇️ OpenClaw not found. Installing OpenClaw (PowerShell)...".to_string());

        let install = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "iwr -useb https://openclaw.ai/install.ps1 | iex",
            ])
            .output();

        match install {
            Ok(res) => {
                if res.status.success() {
                    log.push("✅ OpenClaw installed successfully.".to_string());
                    let out = clean_ansi(&res.stdout);
                    if !out.trim().is_empty() {
                        log.push(out);
                    }
                } else {
                    log.push("❌ OpenClaw install failed.".to_string());
                    let err = clean_ansi(&res.stderr);
                    if !err.trim().is_empty() {
                        log.push(err.clone());
                    }
                    write_log("ERROR", "Setup failed: OpenClaw install failed");
                    return log.join("\n");
                }
            }
            Err(e) => {
                log.push(format!("❌ Failed to run installer: {}", e));
                write_log("ERROR", &format!("Setup failed: installer error {}", e));
                return log.join("\n");
            }
        }
    } else {
        log.push("✅ OpenClaw is already installed.".to_string());
    }

    log.push("🧩 Running: openclaw onboard --install-daemon".to_string());
    let onboard = Command::new("cmd")
        .args(["/C", "openclaw onboard --install-daemon"])
        .output();

    match onboard {
        Ok(res) => {
            if res.status.success() {
                log.push("✅ Onboarding completed.".to_string());
                let out = clean_ansi(&res.stdout);
                if !out.trim().is_empty() {
                    log.push(out);
                }
            } else {
                log.push("⚠️ Onboarding returned an error (may need permissions).".to_string());
                let err = clean_ansi(&res.stderr);
                if !err.trim().is_empty() {
                    log.push(err.clone());
                }
                write_log("WARN", "Onboarding returned error (may need permissions)");
            }
        }
        Err(e) => {
            log.push(format!("❌ Failed to run onboarding: {}", e));
            write_log("ERROR", &format!("Failed to run onboarding: {}", e));
        }
    }

    log.push("✅ Setup finished. Next: connect channels + create agents.".to_string());
    write_log("INFO", "Setup OpenClaw finished");
    log.join("\n")
}

// ------------------------
// 3) Security audit command
// ------------------------
#[tauri::command]
fn openclaw_security_audit() -> String {
    write_log("INFO", "Security audit executed");

    let output = Command::new("cmd")
        .args(["/C", "openclaw security audit --deep"])
        .output();

    match output {
        Ok(res) => {
            if res.stdout.is_empty() {
                let err = clean_ansi(&res.stderr);
                if !err.trim().is_empty() {
                    write_log("ERROR", &format!("Audit error output: {}", err));
                }
                err
            } else {
                let out = clean_ansi(&res.stdout);
                write_log("INFO", "Audit completed");
                out
            }
        }
        Err(e) => {
            let err = format!("Failed to run audit: {}", e);
            write_log("ERROR", &err);
            err
        }
    }
}

// ------------------------
// 4) "Yes, continue" flow
// ------------------------
#[tauri::command]
fn openclaw_finish_onboarding() -> String {
    write_log("INFO", "User approved security continuation");

    let mut out: Vec<String> = vec![];
    out.push("✅ Consent recorded. Continuing with safe setup steps...".to_string());

    out.push("🔍 Running: openclaw security audit --deep".to_string());
    let audit = Command::new("cmd")
        .args(["/C", "openclaw security audit --deep"])
        .output();

    match audit {
        Ok(res) => {
            if res.stdout.is_empty() {
                let err = clean_ansi(&res.stderr);
                if !err.trim().is_empty() {
                    out.push(err.clone());
                    write_log("ERROR", &format!("Finish onboarding audit error: {}", err));
                }
            } else {
                let out_clean = clean_ansi(&res.stdout);
                out.push(out_clean);
                write_log("INFO", "Finish onboarding audit completed");
            }
        }
        Err(e) => {
            out.push(format!("⚠️ Could not run audit: {}", e));
            write_log("ERROR", &format!("Could not run audit: {}", e));
        }
    }

    out.push("🪟 Windows detected: OpenClaw recommends running via WSL2 for best reliability.".to_string());
    out.push("➡️ Run this once in terminal: wsl --install".to_string());
    out.push("After reboot, rerun: setup openclaw".to_string());

    out.join("\n")
}

// ------------------------
// ✅ Gemini API (uses DB key)
// ------------------------
fn redact_secrets(s: &str) -> String {
    let mut out = s.to_string();
    if out.contains("AIza") {
        out = out.replace("AIza", "AIza***REDACTED***");
    }
    out
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiCandidateContent,
}

#[derive(Deserialize)]
struct GeminiCandidateContent {
    parts: Vec<GeminiCandidatePart>,
}

#[derive(Deserialize)]
struct GeminiCandidatePart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiError {
    message: Option<String>,
}

async fn gemini_generate_with_key(key: &str, prompt: &str) -> Result<String, String> {
    let model = "gemini-1.5-flash";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, key
    );

    let body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart {
                text: prompt.to_string(),
            }],
        }],
    };

    let client = reqwest::Client::new();

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gemini request failed: {}", e))?;

    let status = resp.status();
    let text_body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading Gemini response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Gemini HTTP {}: {}",
            status.as_u16(),
            redact_secrets(&text_body)
        ));
    }

    let parsed: GeminiResponse = serde_json::from_str(&text_body)
        .map_err(|e| format!("Failed parsing Gemini JSON: {} | body={}", e, redact_secrets(&text_body)))?;

    if let Some(err) = parsed.error {
        return Err(format!(
            "Gemini error: {}",
            err.message.unwrap_or("Unknown error".to_string())
        ));
    }

    let answer = parsed
        .candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content.parts.get(0).and_then(|p| p.text.clone()))
        .unwrap_or_else(|| "(No response from Gemini)".to_string());

    Ok(answer)
}

// ------------------------
// ✅ LLM Router: local phi3 -> Gemini (if key exists)
// ------------------------
#[tauri::command]
async fn llm_reply(prompt: String) -> Result<String, String> {
    if let Some(key) = get_saved_api_key() {
        write_log("INFO", "LLM routing: external (gemini)");
        let ans = gemini_generate_with_key(&key, &prompt)
            .await
            .map_err(|e| format!("(LLM: gemini) Error: {}", e))?;
        return Ok(format!("(LLM: gemini)\n{}", ans));
    }

    write_log("INFO", "LLM routing: local_phi3");

    let client = reqwest::Client::new();

    let body = json!({
      "model": "phi3",
      "prompt": prompt,
      "stream": false
    });

    let res = client
        .post("http://localhost:11434/api/generate")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Local LLM (Ollama) request failed: {}", e))?;

    let status = res.status();
    let raw_text = res
        .text()
        .await
        .map_err(|e| format!("Local LLM read body failed: {}", e))?;

    if !status.is_success() {
        write_log("ERROR", &format!("Local LLM HTTP {}: {}", status, raw_text));
        return Err(format!("Local LLM HTTP {}:\n{}", status, raw_text));
    }

    let val: serde_json::Value =
        serde_json::from_str(&raw_text).map_err(|e| format!("Parse failed: {}\nRaw:\n{}", e, raw_text))?;

    let text = val["response"].as_str().unwrap_or("").to_string();

    if text.trim().is_empty() {
        write_log("ERROR", "Local LLM returned empty content");
        return Err(format!("Local LLM returned empty content.\nRaw:\n{}", raw_text));
    }

    Ok(format!("(LLM: local_phi3)\n{}", text))
}

// ------------------------
// ✅ Commands used by UI
// ------------------------
#[tauri::command]
fn set_llm_key(llm_api_key: String, llm_provider: String) -> Result<String, String> {
    save_user_api_key(llm_api_key, llm_provider)
}

#[tauri::command]
fn show_settings() -> Result<String, String> {
    get_user_settings()
}

fn main() {
    // Ensure DB exists at boot
    let _ = open_db().map(|conn| {
        ensure_logs_table(&conn);
        ensure_user_settings_table(&conn);
    });

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            send_message,
            setup_openclaw,
            openclaw_security_audit,
            openclaw_finish_onboarding,
            llm_reply,
            save_user_api_key,
            set_llm_key,
            show_settings,
            get_user_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
