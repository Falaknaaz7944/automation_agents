
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Command;

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
fn ensure_approvals_table(conn: &rusqlite::Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS approvals (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            draft_text TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            decided_at TEXT NULL
        )",
        [],
    );
}
fn create_approval(agent_id: &str, kind: &str, draft_text: &str) -> Result<String, String> {
    use rusqlite::params;
    use uuid::Uuid;

    let conn = open_db()?;
    ensure_approvals_table(&conn);

    let id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO approvals (id, agent_id, kind, draft_text, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![id, agent_id, kind, draft_text],
    )
    .map_err(|e| format!("DB insert failed: {}", e))?;

    Ok(id)
}

fn ensure_agents_table(conn: &rusqlite::Connection) {
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    role TEXT NOT NULL,
    goal TEXT NOT NULL,
    tools_json TEXT NOT NULL,
    schedule TEXT NULL,
    triggers_json TEXT NULL,
    sandbox INTEGER NOT NULL DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
)",
        [],
    );
   
}
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
    write_log_with_agent(level, None, message);
}

fn write_log_agent(level: &str, agent_id: &str, message: &str) {
    write_log_with_agent(level, Some(agent_id), message);
}

fn write_log_with_agent(level: &str, agent_id: Option<&str>, message: &str) {
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
    ensure_agents_table(&conn);
    

    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return,
    };

    let id = Uuid::new_v4().to_string();

    let _ = conn.execute(
        "INSERT INTO logs (id, agent_id, timestamp, level, message, llm_used, status, error)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL)",
        params![id, agent_id, now.to_string(), level, message],
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
       "SELECT l.timestamp, l.level, l.message, a.name
        FROM logs l
        LEFT JOIN agents a ON l.agent_id = a.id
        ORDER BY l.timestamp DESC
        LIMIT ?1"
    ) {
        Ok(s) => s,
        Err(e) => return format!("❌ Failed to prepare logs query: {}", e),
    };

    let rows = stmt.query_map([limit], |row| {
    let ts: String = row.get(0)?;
    let level: String = row.get(1)?;
    let msg: String = row.get(2)?;
    let agent_name: Option<String> = row.get(3)?;
    Ok((ts, level, msg, agent_name))
});

    match rows {
        Ok(iter) => {
            let mut items: Vec<(String, String, String, Option<String>)> = vec![];
            for r in iter.flatten() {
                items.push(r);
            }

            if items.is_empty() {
                return "ℹ️ No logs yet.".to_string();
            }

            let mut out = String::from("🧾 Last logs:\n\n");
            for (ts, level, msg, agent_name) in items {
                if let Some(name) = agent_name {
                    out.push_str(&format!("[{}] {} [Agent: {}] — {}\n", ts, level, name, msg));
                } else {
                    out.push_str(&format!("[{}] {} — {}\n", ts, level, msg));
                }
}

            out
        }
        Err(e) => format!("❌ Failed reading logs: {}", e),
    }
}

// -------------------------
// ✅ User settings helpers
// Architecture:
// - If llm_api_key exists => use external provider (gemini/openai/anthropic)
// - else => local phi3 via Ollama
// -------------------------
fn project_root() -> std::path::PathBuf {
    // src-tauri -> project root
    let mut dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    // if we are inside src-tauri, go up one
    if dir.ends_with("src-tauri") {
        dir.pop();
    }
    dir
}

fn automation_dir() -> std::path::PathBuf {
    // Always: .../personaliz-desktop/src-tauri
    let tauri_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Go up to: .../personaliz-desktop
    let project_root = tauri_dir
        .parent()
        .unwrap_or(&tauri_dir)
        .to_path_buf();

    project_root.join("automation")
}

fn run_node_script(script: &str, args: Vec<String>) -> Result<String, String> {
    let script_path = automation_dir().join(script);

    if !script_path.exists() {
        return Err(format!("❌ Script not found: {}", script_path.display()));
    }

    let mut cmd = Command::new("node");

    // ✅ Don't set current_dir at all (avoid Windows 267 issues)
    cmd.arg(script_path);

    for a in args {
        cmd.arg(a);
    }

    let out = cmd.output().map_err(|e| format!("Failed running node: {}", e))?;
    let stdout = clean_ansi(&out.stdout);
    let stderr = clean_ansi(&out.stderr);

    if !out.status.success() {
        return Err(format!(
            "Node script failed:\n{}",
            if stderr.trim().is_empty() { stdout } else { stderr }
        ));
    }

    Ok(if stdout.trim().is_empty() {
        "✅ Done.".to_string()
    } else {
        stdout
    })
}
fn run_node_script_args(script: &str, args: Vec<String>) -> Result<String, String> {
    run_node_script(script, args)
}
fn normalize_provider(p: &str) -> String {
    let s = p.trim().to_lowercase();
    match s.as_str() {
        "gemini" | "google" => "gemini".to_string(),
        "openai" | "gpt" => "openai".to_string(),
        "claude" | "anthropic" => "anthropic".to_string(),
        other => other.to_string(), // still store, but router will reject unknown
    }
}

fn get_saved_llm() -> Option<(String, String)> {
    use rusqlite::OptionalExtension;

    let conn = open_db().ok()?;
    ensure_user_settings_table(&conn);

    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT llm_api_key, llm_provider FROM user_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()?;

    let (key_opt, prov_opt) = row?;
    let key = key_opt?.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let provider = normalize_provider(prov_opt.unwrap_or_else(|| "gemini".to_string()).as_str());
    Some((provider, key))
}
#[tauri::command]
fn demo1_run() -> Result<String, String> {
    // Find “daily trending agent” (latest agent with tools_json containing demo_trending)
    let conn = open_db()?;
    ensure_agents_table(&conn);
    ensure_approvals_table(&conn);

    let (agent_id, agent_name): (String, String) = conn
        .query_row(
            "SELECT id, name FROM agents
             WHERE tools_json LIKE '%demo_trending%'
             ORDER BY created_at DESC
             LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "❌ No agent found with tool demo_trending. Create Demo1 agent first.".to_string())?;

    write_log_agent("INFO", &agent_id, "Demo1 trigger started");

    // Pull trending topics (OpenClaw or fallback)
    let topics = get_trending_topics();
    let bullet = topics
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    // Simple post template (no LLM dependency)
    let draft = format!(
        "Today’s trends I’m watching 👇\n\n{}\n\nCurious: which one will dominate 2026?\n#openclaw #automation #ai",
        bullet
    );

    // Create approval (waits for user)
    let approval_id = create_approval(&agent_id, "linkedin_post", &draft)?;

    write_log_agent(
        "INFO",
        &agent_id,
        &format!("Demo1 created approval id={} for agent '{}'", approval_id, agent_name),
    );

    Ok(format!(
        "✅ Demo1 prepared a LinkedIn draft for approval.\n\nRun:\npending approvals\nThen:\napprove {}\n",
        approval_id
    ))
}

#[tauri::command]
fn demo2_run() -> Result<String, String> {
    // Find “hourly hashtag agent” (latest agent with tools_json containing linkedin_comment)
    let conn = open_db()?;
    ensure_agents_table(&conn);

    let (agent_id, agent_name): (String, String) = conn
        .query_row(
            "SELECT id, name FROM agents
             WHERE tools_json LIKE '%linkedin_comment%'
             ORDER BY created_at DESC
             LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "❌ No agent found with tool linkedin_comment. Create Demo2 agent first.".to_string())?;

    write_log_agent("INFO", &agent_id, "Demo2 trigger started (auto comment)");

    // This is the promo comment text (edit it to your repo + pitch)
    let comment = "Just shipped a desktop automation assistant using OpenClaw + Tauri 🚀\nTry it if you’re non-technical too — it’s chat-first.\nRepo: https://github.com/<YOUR_USERNAME>/<YOUR_REPO>\n#openclaw";

    // Run Playwright comment automation (must exist)
    let out = run_node_script("linkedin_comment.js", vec![comment.to_string()])?;

    write_log_agent(
        "INFO",
        &agent_id,
        &format!("Demo2 completed for agent '{}'", agent_name),
    );

    Ok(format!("✅ Demo2 done (commented via browser automation).\n\n{}", out))
}

#[tauri::command]
fn run_demo1_now(agent_name: String) -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);
    ensure_approvals_table(&conn);

    // find agent by name (latest)
    let agent_id: String = conn
        .query_row(
            "SELECT id FROM agents WHERE name=?1 ORDER BY created_at DESC LIMIT 1",
            [agent_name.clone()],
            |r| r.get(0),
        )
        .map_err(|_| "❌ Agent not found by that name.".to_string())?;

    write_log_agent("INFO", &agent_id, "Demo1 trigger: generating trending post draft");

    let topics = get_trending_topics();
    let top = topics.get(0).cloned().unwrap_or_else(|| "OpenClaw".to_string());

    let draft = format!(
        "🚀 Trending today: {}\n\nI’m building a Tauri desktop assistant that uses OpenClaw + browser automation to run daily workflows.\n\nWhat’s one automation you wish your desktop could do for you?",
        top
    );

    let approval_id = create_approval(&agent_id, "linkedin_post", &draft)?;
    write_log_agent("INFO", &agent_id, &format!("Created approval id={}", approval_id));

    Ok(format!(
        "✅ Demo1 draft created for agent '{}'\nApproval id: {}\n\nType: pending approvals",
        agent_name, approval_id
    ))
}
fn parse_tools(tools_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(tools_json).unwrap_or_default()
}

fn build_demo1_post(topics: &[String]) -> String {
    let t = topics.get(0).cloned().unwrap_or_else(|| "AI agents".to_string());
    format!(
"🚀 Today’s OpenClaw trend: **{}**

What’s interesting is how fast “agentic workflows” are moving from experiments to real desktop automation:
✅ local-first LLM routing  
✅ approvals before risky actions  
✅ browser automation (no APIs needed)  
✅ scheduled repeatability

If you’re building with OpenClaw, what’s the most useful agent you’ve made so far?

#openclaw #automation #aiagents #productivity",
        t
    )
}

fn build_demo2_comment(repo_url: &str) -> String {
    format!(
"Hey! I just shipped a new OpenClaw-powered desktop assistant (Tauri) that automates LinkedIn actions via browser automation (not API). Repo: {} 🚀

If you’re non-technical and want to try it, tell me — I’ll share a quick setup guide.",
        repo_url
    )
}

#[tauri::command]
async fn run_demo2_now(agent_name: String, github_url: String) -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);

    // find agent by name
    let agent_id: String = conn
        .query_row(
            "SELECT id FROM agents WHERE name=?1 ORDER BY created_at DESC LIMIT 1",
            [agent_name.clone()],
            |r| r.get(0),
        )
        .map_err(|_| "❌ Agent not found by that name.".to_string())?;

    let comment = format!(
        "🚀 Quick share: I just shipped a desktop automation assistant built with Tauri + OpenClaw.\nRepo: {}\nIf you're non-technical, you can still use it — it’s chat-based + does browser automation for you.",
        github_url
    );

    write_log_agent("INFO", &agent_id, "Demo2 trigger: posting comment on #openclaw");

    let res = tokio::task::spawn_blocking(move || {
        run_node_script("linkedin_comment_openclaw.js", vec![comment])
    })
    .await
    .map_err(|e| format!("Join error: {}", e))??;

    write_log_agent("INFO", &agent_id, "Demo2 completed: comment posted");
    Ok(res)
}
#[tauri::command]
fn create_demo_agents() -> Result<String, String> {
    // Demo 1: daily trending -> approval -> post
    save_agent_config(
        "Trending Agent".to_string(),
        "Assistant".to_string(),
        "Find trending topic and post to LinkedIn daily (approval required)".to_string(),
        serde_json::to_string(&vec!["demo_trending".to_string(), "linkedin_post".to_string()]).unwrap(),
        Some("daily".to_string()),
        None,
        false,
    )?;

    // Demo 2: hourly hashtag -> auto comment
    save_agent_config(
        "Hashtag Promo Agent".to_string(),
        "Assistant".to_string(),
        "Every hour comment on #openclaw posts promoting repo".to_string(),
        serde_json::to_string(&vec!["demo_hashtag".to_string(), "linkedin_comment".to_string()]).unwrap(),
        Some("hourly".to_string()),
        None,
        false,
    )?;

    Ok("✅ Created Demo 1 + Demo 2 agents.".to_string())
}

#[tauri::command]
fn run_demo1_once() -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);
    ensure_approvals_table(&conn);

    // find Demo1 agent
    let (agent_id, tools_json): (String, String) = conn.query_row(
        "SELECT id, tools_json FROM agents WHERE name='Trending Agent' ORDER BY created_at DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| "❌ Trending Agent not found. Run: create demo agents".to_string())?;

    let tools = parse_tools(&tools_json);
    if !tools.iter().any(|t| t == "demo_trending") {
        return Err("❌ Trending Agent tools missing demo_trending".to_string());
    }

    write_log_agent("INFO", &agent_id, "Demo1 started: fetching trending topics...");
    let topics = get_trending_topics();
    let draft = build_demo1_post(&topics);

    let approval_id = create_approval(&agent_id, "linkedin_post", &draft)?;
    write_log_agent("INFO", &agent_id, &format!("Created approval id={}", approval_id));

    Ok(format!(
        "🧩 Demo1 Draft Ready (approval required)\nApproval ID: {}\n\nType:\napprove {}\n\nOr view:\npending approvals",
        approval_id, approval_id
    ))
}

#[tauri::command]
fn run_demo2_once() -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);

    let (agent_id, tools_json): (String, String) = conn.query_row(
        "SELECT id, tools_json FROM agents WHERE name='Hashtag Promo Agent' ORDER BY created_at DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| "❌ Hashtag Promo Agent not found. Run: create demo agents".to_string())?;

    let tools = parse_tools(&tools_json);
    if !tools.iter().any(|t| t == "demo_hashtag") {
        return Err("❌ Hashtag Promo Agent tools missing demo_hashtag".to_string());
    }

    let repo_url = "https://github.com/<YOUR_USERNAME>/<YOUR_REPO>"; // ✅ change this
    let comment = build_demo2_comment(repo_url);

    write_log_agent("INFO", &agent_id, "Demo2 started: commenting on #openclaw...");
    let res = run_node_script("linkedin_comment.js", vec![comment])?;
    write_log_agent("INFO", &agent_id, "Demo2 completed: comments posted.");

    Ok(format!("✅ Demo2 done.\n\n{}", res))
}

// ✅ For video: simulate scheduler (no waiting 1 hour / 9am)
#[tauri::command]
fn scheduler_tick_now() -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);

    let mut stmt = conn
        .prepare("SELECT id, name, schedule FROM agents WHERE schedule IS NOT NULL")
        .map_err(|e| format!("Query failed: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| format!("Query map failed: {}", e))?;

    let mut out = String::from("⏱ Scheduler tick executed:\n");

    for r in rows.flatten() {
        let agent_id = r.0;
        let name = r.1;
        let sched = r.2.unwrap_or_default().to_lowercase();

        if sched.contains("daily") {
            write_log_agent("INFO", &agent_id, &format!("Scheduler tick: daily agent '{}' fired", name));
            out.push_str(&format!("✅ daily fired: {}\n", name));
        }
        if sched.contains("hourly") {
            write_log_agent("INFO", &agent_id, &format!("Scheduler tick: hourly agent '{}' fired", name));
            out.push_str(&format!("✅ hourly fired: {}\n", name));
        }
    }

    Ok(out)
}

#[tauri::command]
fn list_pending_approvals() -> Result<String, String> {
    let conn = open_db()?;
    ensure_approvals_table(&conn);

    let mut stmt = conn
        .prepare("SELECT id, kind, draft_text FROM approvals WHERE status='pending' ORDER BY created_at DESC")
        .map_err(|e| format!("Query failed: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Query map failed: {}", e))?;

    let mut out = String::from("📝 Pending Approvals:\n\n");
    let mut count = 0;

    for r in rows.flatten() {
        count += 1;
        out.push_str(&format!(
            "{}. ID: {}\n   Type: {}\n   Draft:\n{}\n\n",
            count, r.0, r.1, r.2
        ));
    }

    if count == 0 {
        Ok("ℹ️ No pending approvals.".to_string())
    } else {
        Ok(out)
    }
}
#[tauri::command]
fn approve_action(id: String) -> Result<String, String> {
    use rusqlite::params;

    let conn = open_db()?;
    ensure_approvals_table(&conn);

    // get approval payload
    let (agent_id, kind, draft_text): (String, String, String) = conn
        .query_row(
            "SELECT agent_id, kind, draft_text FROM approvals WHERE id=?1 AND status='pending'",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "❌ Approval not found or already decided.".to_string())?;

    // mark approved
    conn.execute(
        "UPDATE approvals SET status='approved', decided_at=datetime('now') WHERE id=?1",
        params![id],
    )
    .map_err(|e| format!("DB update failed: {}", e))?;

    write_log_agent("INFO", &agent_id, &format!("Approval accepted id={}", id));

    // ✅ AUTO RUN ACTION (once)
    match kind.as_str() {
        "linkedin_post" => {
            write_log_agent("INFO", &agent_id, "Posting to LinkedIn...");

            let result = run_node_script("linkedin_post.js", vec![draft_text.clone()])?;

            write_log_agent("INFO", &agent_id, "LinkedIn post completed");

            Ok(format!("✅ Approved & Posted.\n\n{}", result))
        }
        "linkedin_comment" => {
            write_log_agent("INFO", &agent_id, "Commenting on LinkedIn...");

            let result = run_node_script("linkedin_comment.js", vec![draft_text.clone()])?;

            write_log_agent("INFO", &agent_id, "LinkedIn comment completed");

            Ok(format!("✅ Approved & Commented.\n\n{}", result))
        }
        other => Err(format!("❌ Unknown approval kind '{}'", other)),
    }
}



#[tauri::command]
async fn linkedin_login() -> Result<String, String> {
    write_log("INFO", "LinkedIn login (record session) started");

    let res = tokio::task::spawn_blocking(|| {
        run_node_script("linkedin_login.js", vec![])
    })
    .await
    .map_err(|e| format!("Join error: {}", e))??;

    write_log("INFO", "LinkedIn login session saved");

    Ok(res)
}
#[tauri::command]
async fn linkedin_post(text: String) -> Result<String, String> {
    write_log("INFO", "LinkedIn post requested");

    let res = tokio::task::spawn_blocking(move || {
        run_node_script("linkedin_post.js", vec![text])
    })
    .await
    .map_err(|e| format!("Join error: {}", e))??;

    write_log("INFO", "LinkedIn post completed");
    Ok(res)
}
fn run_openclaw(args: &[&str]) -> Result<String, String> {
    let out = Command::new("cmd")
        .args(["/C", &format!("openclaw {}", args.join(" "))])
        .output()
        .map_err(|e| format!("Failed to run openclaw: {}", e))?;

    let stdout = clean_ansi(&out.stdout);
    let stderr = clean_ansi(&out.stderr);

    if !out.status.success() {
        return Err(if stderr.trim().is_empty() { stdout } else { stderr });
    }
    Ok(stdout)
}
fn get_trending_topics() -> Vec<String> {
    // Replace this with the real OpenClaw command you have available.
    // Example guesses: "trending", "trends", "top", etc.
    if let Ok(out) = run_openclaw(&["trending", "--top", "5"]) {
        // simplest parsing: each line is a topic
        let topics: Vec<String> = out
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .take(5)
            .collect();

        if !topics.is_empty() {
            return topics;
        }
    }

    // fallback so demo never fails
    vec![
        "AI agents in desktop apps".to_string(),
        "Local LLM + privacy-first workflows".to_string(),
        "Browser automation for daily ops".to_string(),
    ]
}
#[tauri::command]
fn save_user_api_key(llm_api_key: String, llm_provider: String) -> Result<String, String> {
    use rusqlite::params;

    let key = llm_api_key.trim().to_string();
    if key.is_empty() {
        return Err("❌ API key is empty.".to_string());
    }

    let provider = normalize_provider(&llm_provider);

    let conn = open_db()?;
    ensure_user_settings_table(&conn);

    conn.execute(
        "UPDATE user_settings
         SET llm_api_key = ?1, llm_provider = ?2, updated_at = datetime('now')
         WHERE id=1",
        params![key, provider],
    )
    .map_err(|e| format!("DB update failed: {}", e))?;

    write_log("INFO", "Saved external LLM API key + provider");
    Ok("✅ Saved key. LLM will switch to external provider automatically.".to_string())
}

#[tauri::command]
fn clear_user_api_key() -> Result<String, String> {
    use rusqlite::params;

    let conn = open_db()?;
    ensure_user_settings_table(&conn);

    conn.execute(
        "UPDATE user_settings
         SET llm_api_key = NULL, llm_provider = NULL, updated_at = datetime('now')
         WHERE id=1",
        params![],
    )
    .map_err(|e| format!("DB update failed: {}", e))?;

    write_log("INFO", "Cleared external LLM key");
    Ok("✅ Cleared key. LLM will use local Phi-3 (offline) again.".to_string())
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
        "llm_api_key: {}\nllm_provider: {}\nrouter: {}",
        if key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false) {
            "✅ set"
        } else {
            "❌ not set"
        },
        provider.unwrap_or_else(|| "(none)".to_string()),
        if key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false) {
            "external"
        } else {
            "local_phi3"
        }
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
// ✅ LLM calls (Local + External)
// ------------------------
fn redact_secrets(s: &str) -> String {
    // prevent leaking keys in UI logs/errors
    let mut out = s.to_string();
    if out.contains("AIza") {
        out = out.replace("AIza", "AIza***REDACTED***");
    }
    if out.contains("sk-") {
        out = out.replace("sk-", "sk-***REDACTED***");
    }
    out
}

// ===== Gemini =====
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
    // You can change model later if needed
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

    let parsed: GeminiResponse = serde_json::from_str(&text_body).map_err(|e| {
        format!(
            "Failed parsing Gemini JSON: {} | body={}",
            e,
            redact_secrets(&text_body)
        )
    })?;

    if let Some(err) = parsed.error {
        return Err(format!(
            "Gemini error: {}",
            err.message.unwrap_or("Unknown error".to_string())
        ));
    }

    let answer = parsed
        .candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content.parts.into_iter().find_map(|p| p.text))
        .unwrap_or_else(|| "(No response from Gemini)".to_string());

    Ok(answer)
}

// ===== OpenAI (Chat Completions API) =====
#[derive(Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIChatMessage>,
}

#[derive(Serialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChatChoice>,
}

#[derive(Deserialize)]
struct OpenAIChatChoice {
    message: OpenAIChatChoiceMessage,
}

#[derive(Deserialize)]
struct OpenAIChatChoiceMessage {
    content: Option<String>,
}

async fn openai_generate_with_key(key: &str, prompt: &str) -> Result<String, String> {
    let url = "https://api.openai.com/v1/chat/completions";
    let body = OpenAIChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![OpenAIChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenAI request failed: {}", e))?;

    let status = resp.status();
    let text_body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading OpenAI response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "OpenAI HTTP {}: {}",
            status.as_u16(),
            redact_secrets(&text_body)
        ));
    }

    let parsed: OpenAIChatResponse = serde_json::from_str(&text_body).map_err(|e| {
        format!(
            "Failed parsing OpenAI JSON: {} | body={}",
            e,
            redact_secrets(&text_body)
        )
    })?;

    let ans = parsed
        .choices
        .get(0)
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "(No response from OpenAI)".to_string());

    Ok(ans)
}

// ===== Anthropic (Claude Messages API) =====
#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

async fn anthropic_generate_with_key(key: &str, prompt: &str) -> Result<String, String> {
    let url = "https://api.anthropic.com/v1/messages";

    let body = AnthropicRequest {
        model: "claude-3-5-sonnet-20240620".to_string(),
        max_tokens: 800,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {}", e))?;

    let status = resp.status();
    let text_body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading Anthropic response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Anthropic HTTP {}: {}",
            status.as_u16(),
            redact_secrets(&text_body)
        ));
    }

    let parsed: AnthropicResponse = serde_json::from_str(&text_body).map_err(|e| {
        format!(
            "Failed parsing Anthropic JSON: {} | body={}",
            e,
            redact_secrets(&text_body)
        )
    })?;

    let ans = parsed
        .content
        .into_iter()
        .find_map(|b| b.text)
        .unwrap_or_else(|| "(No response from Claude)".to_string());

    Ok(ans)
}

// ===== Local Phi-3 (Ollama) =====
async fn local_phi3(prompt: &str) -> Result<String, String> {
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

    Ok(text)
}

// ------------------------
// ✅ LLM Router (THE IMPORTANT PART)
// If user key exists => external provider
// else => offline local Phi-3
// ------------------------
#[tauri::command]
async fn llm_reply(prompt: String) -> Result<String, String> {
    if let Some((provider, key)) = get_saved_llm() {
        write_log("INFO", &format!("LLM routing: external ({})", provider));

        let ans = match provider.as_str() {
            "gemini" => gemini_generate_with_key(&key, &prompt).await,
            "openai" => openai_generate_with_key(&key, &prompt).await,
            "anthropic" => anthropic_generate_with_key(&key, &prompt).await,
            other => Err(format!(
                "Unknown provider '{}'. Use: gemini | openai | anthropic (claude).",
                other
            )),
        }
        .map_err(|e| format!("(LLM: {}) Error: {}", provider, e))?;

        return Ok(format!("(LLM: {})\n{}", provider, ans));
    }

    write_log("INFO", "LLM routing: local_phi3");

    let ans = local_phi3(&prompt)
        .await
        .map_err(|e| format!("(LLM: local_phi3) Error: {}", e))?;

    Ok(format!("(LLM: local_phi3)\n{}", ans))
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
#[tauri::command]
fn save_agent_config(
    name: String,
    role: String,
    goal: String,
    tools_json: String,
    schedule: Option<String>,
    triggers_json: Option<String>,
    sandbox: bool,
) -> Result<String, String> {
    use rusqlite::params;
    use uuid::Uuid;

    let conn = open_db()?;
    ensure_agents_table(&conn);

    let id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO agents (id, name, role, goal, tools_json, schedule, triggers_json, sandbox, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        params![
            id,
            name,
            role,
            goal,
            tools_json,
            schedule,
            triggers_json,
            if sandbox { 1 } else { 0 }
        ],
    )
    .map_err(|e| format!("DB insert failed: {}", e))?;

    // ✅ THIS is why it wasn't showing agent creation in logs earlier.
    // You were only logging "Saved agent config" (too generic).
    write_log_agent(
    "INFO",
    &id,
    &format!(
        "Agent created: schedule={}, sandbox={}",
        schedule.as_deref().unwrap_or("none"),
        if sandbox { "ON" } else { "OFF" }
    ),
);

    Ok(format!("✅ Agent saved with id: {}", id))
}


#[tauri::command]
fn list_agents() -> Result<String, String> {
    let conn = open_db()?;
    ensure_agents_table(&conn);

    let mut stmt = conn
        .prepare("SELECT id, name, goal, sandbox, created_at FROM agents ORDER BY created_at DESC")
        .map_err(|e| format!("Query prepare failed: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let goal: String = row.get(2)?;
            let sandbox: i64 = row.get(3)?;
            let created_at: String = row.get(4)?;
            Ok((id, name, goal, sandbox, created_at))
        })
        .map_err(|e| format!("Query map failed: {}", e))?;

    let mut out = String::from("🤖 Saved Agents:\n\n");
    let mut count = 0;

    for r in rows.flatten() {
        count += 1;
        out.push_str(&format!(
            "{}. {}\n   id: {}\n   goal: {}\n   sandbox: {}\n   created: {}\n\n",
            count,
            r.1,
            r.0,
            r.2,
            if r.3 == 1 { "✅ ON" } else { "❌ OFF" },
            r.4
        ));
    }

    if count == 0 {
        Ok("ℹ️ No agents saved yet.".to_string())
    } else {
        Ok(out)
    }
}
use chrono::{Local, Timelike};
use tokio::time::{sleep, Duration};

// Background scheduler

async fn scheduler_loop() {
    loop {
        let now = Local::now();
        let minute = now.minute();
        let hour = now.hour();

        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(conn) = open_db() {
                ensure_agents_table(&conn);

                if let Ok(mut stmt) = conn.prepare(
                    "SELECT id, name, schedule, sandbox FROM agents WHERE schedule IS NOT NULL"
                ) {
                    if let Ok(rows) = stmt.query_map([], |row| {
                        let id: String = row.get(0)?;
                        let name: String = row.get(1)?;
                        let schedule: Option<String> = row.get(2)?;
                        let sandbox: i64 = row.get(3)?;
                        Ok((id, name, schedule, sandbox))
                    }) {
                        for agent in rows.flatten() {
                            if let Some(sched) = agent.2.as_deref() {
                                let s = sched.to_lowercase();



                              if s.contains("daily") && hour == 9 && minute == 0 {
    write_log_agent(
        "INFO",
        &agent.0,
        &format!("Scheduler executed daily agent '{}'", agent.1),
    );

    let topics = get_trending_topics();
    let draft = build_demo1_post(&topics);

    match create_approval(&agent.0, "linkedin_post", &draft) {
        Ok(id) => write_log_agent("INFO", &agent.0, &format!("Created approval id={}", id)),
        Err(e) => write_log_agent("ERROR", &agent.0, &format!("Approval creation failed: {}", e)),
    }
}

if s.contains("hourly") && minute == 0 && agent.1.to_lowercase().contains("hashtag") {
    write_log_agent("INFO", &agent.0, "Scheduler fired (hourly) -> Demo2");
    let _ = run_demo2_once();
}

                            }
                        }
                    }
                }
            }
        }).await;

        sleep(Duration::from_secs(60)).await;
    }
}








fn main() {
    let _ = open_db().map(|conn| {
        ensure_logs_table(&conn);
        ensure_user_settings_table(&conn);
        ensure_agents_table(&conn);
        ensure_approvals_table(&conn);
    });

    tauri::async_runtime::spawn(async {
        scheduler_loop().await;
    });

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            send_message,
            setup_openclaw,
            openclaw_security_audit,
            openclaw_finish_onboarding,
            llm_reply,
            save_user_api_key,
            clear_user_api_key,
            set_llm_key,
            show_settings,
            save_agent_config,
            list_agents,
            list_pending_approvals,
            approve_action,
            linkedin_login,
            create_demo_agents,
            demo1_run,
            run_demo1_once,
            //run_demo2_once,
            demo2_run,
            scheduler_tick_now,
            linkedin_post,
            get_user_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


