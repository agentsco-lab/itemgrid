//! hythe-mcp: Hythe's window for Claude Code to look at and try - an MCP
//! server on stdio (JSON-RPC a line). It starts Hythe with its control
//! socket (HYTHE_CONTROL=1, crates/gui/src/control.rs) and asks it for a
//! picture of the window and its state; input is the real thing - the
//! pointer moved and its buttons and wheel pressed on the screen (xdotool,
//! X11), so the window's own gestures are what is tried. Coordinates are
//! the window's, as in its picture.
//!
//! `hythe-mcp call TOOL [JSON]`: a tool from the command line (a picture
//! is left at its path).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn runtime_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(Into::into).unwrap_or_else(std::env::temp_dir)
}

fn socket() -> std::path::PathBuf {
    runtime_dir().join("hythe-control.sock")
}

fn trace_log() -> std::path::PathBuf {
    runtime_dir().join("hythe-trace.log")
}

/// A request to the window, its answer.
fn ask(cmd: &str) -> Result<Value, String> {
    let mut s = UnixStream::connect(socket()).map_err(|_| "Hythe is not running with its control socket: start it (start, restart: true)".to_owned())?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    writeln!(s, "{}", json!({ "cmd": cmd })).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    match v.get("error") {
        Some(e) => Err(e.to_string()),
        None => Ok(v),
    }
}

fn xdotool(args: &[String]) -> Result<(), String> {
    let out = Command::new("xdotool").args(args).output().map_err(|e| format!("xdotool: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("xdotool {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The window brought to the front, and where its content is.
fn window_origin() -> Result<(f64, f64), String> {
    let _ = Command::new("xdotool").args(["search", "--onlyvisible", "--name", "^Hythe$", "windowactivate", "--sync"]).output();
    std::thread::sleep(Duration::from_millis(80));
    let state = ask("state")?;
    let o = &state["window"]["content_on_screen"];
    match (o[0].as_f64(), o[1].as_f64()) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err("the window's place is not known (not on X11?)".into()),
    }
}

fn at(origin: (f64, f64), x: f64, y: f64) -> [String; 2] {
    [format!("{}", (origin.0 + x).round() as i64), format!("{}", (origin.1 + y).round() as i64)]
}

fn num(args: &Value, key: &str) -> Result<f64, String> {
    args[key].as_f64().ok_or_else(|| format!("'{key}': a number"))
}

fn point(args: &Value, key: &str) -> Result<(f64, f64), String> {
    match (args[key][0].as_f64(), args[key][1].as_f64()) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(format!("'{key}': [x, y]")),
    }
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What a tool gives back: words, and a picture.
struct Answer {
    text: String,
    picture: Option<std::path::PathBuf>,
}

fn words(text: impl Into<String>) -> Answer {
    Answer { text: text.into(), picture: None }
}

fn picture() -> Result<Answer, String> {
    let v = ask("shot")?;
    let path = v["path"].as_str().ok_or("no picture")?.into();
    Ok(Answer { text: format!("{} x {}", v["width"], v["height"]), picture: Some(path) })
}

/// After an input: a picture if asked for, a moment later.
fn then_picture(args: &Value, done: String) -> Result<Answer, String> {
    if args["screenshot"].as_bool().unwrap_or(false) {
        std::thread::sleep(Duration::from_millis(args["settle_ms"].as_u64().unwrap_or(300)));
        let mut a = picture()?;
        a.text = format!("{done}; {}", a.text);
        Ok(a)
    } else {
        Ok(words(done))
    }
}

fn start(args: &Value) -> Result<Answer, String> {
    let running = Command::new("pgrep").args(["-x", "hythe-gui"]).output().is_ok_and(|o| o.status.success());
    if running && ask("state").is_ok() && !args["restart"].as_bool().unwrap_or(false) {
        return Ok(words("already running with its control socket"));
    }
    if running {
        let _ = Command::new("pkill").args(["-x", "hythe-gui"]).status();
        std::thread::sleep(Duration::from_millis(700));
    }
    let gui = std::env::var("HYTHE_GUI").unwrap_or_else(|_| format!("{}/.local/bin/hythe-gui", std::env::var("HOME").unwrap_or_default()));
    let log = std::fs::File::create(trace_log()).map_err(|e| e.to_string())?;
    let mut c = Command::new(&gui);
    c.env("HYTHE_CONTROL", "1").env("HYTHE_TRACE", "1").stdin(Stdio::null()).stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log);
    if let Some(intro) = args["intro"].as_bool() {
        if !intro {
            c.env("HYTHE_INTRO", "0");
        }
    }
    use std::os::unix::process::CommandExt;
    c.process_group(0);
    c.spawn().map_err(|e| format!("{gui}: {e}"))?;
    let since = Instant::now();
    while since.elapsed() < Duration::from_secs(15) {
        if ask("state").is_ok() {
            return Ok(words(format!("started in {:.1} s (trace: {})", since.elapsed().as_secs_f32(), trace_log().display())));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("started, but its control socket did not answer within 15 s".into())
}

fn call(name: &str, args: &Value) -> Result<Answer, String> {
    match name {
        "start" => start(args),
        "screenshot" => picture(),
        "state" => Ok(words(serde_json::to_string_pretty(&ask("state")?).unwrap_or_default())),
        "replay" => {
            ask("replay")?;
            Ok(words("the start plays again"))
        }
        "trace" => {
            let n = args["lines"].as_u64().unwrap_or(40) as usize;
            let text = std::fs::read_to_string(trace_log()).map_err(|e| e.to_string())?;
            let lines: Vec<&str> = text.lines().collect();
            Ok(words(lines[lines.len().saturating_sub(n)..].join("\n")))
        }
        "wait" => {
            std::thread::sleep(Duration::from_millis(args["ms"].as_u64().unwrap_or(500).min(30_000)));
            if args["screenshot"].as_bool().unwrap_or(false) {
                picture()
            } else {
                Ok(words("waited"))
            }
        }
        "click" => {
            let o = window_origin()?;
            let (x, y) = (num(args, "x")?, num(args, "y")?);
            let button = args["button"].as_u64().unwrap_or(1).to_string();
            let mut a: Vec<String> = vec!["mousemove".into(), "--sync".into()];
            a.extend(at(o, x, y));
            a.extend(["click".into(), "--repeat".into(), if args["double"].as_bool().unwrap_or(false) { "2" } else { "1" }.into(), button]);
            xdotool(&a)?;
            then_picture(args, format!("clicked at {x}, {y}"))
        }
        "scroll" => {
            let o = window_origin()?;
            let (x, y) = (num(args, "x")?, num(args, "y")?);
            let clicks = num(args, "clicks")? as i64;
            if clicks == 0 {
                return Ok(words("nothing to scroll"));
            }
            let mut a: Vec<String> = vec!["mousemove".into(), "--sync".into()];
            a.extend(at(o, x, y));
            let button = if clicks > 0 { "5" } else { "4" };
            a.extend(["click".into(), "--repeat".into(), clicks.abs().to_string(), "--delay".into(), args["delay_ms"].as_u64().unwrap_or(40).to_string(), button.into()]);
            xdotool(&a)?;
            then_picture(args, format!("scrolled {clicks} at {x}, {y} (positive: down)"))
        }
        "drag" => {
            let o = window_origin()?;
            let (from, to) = (point(args, "from")?, point(args, "to")?);
            let ms = args["ms"].as_u64().unwrap_or(600).max(50);
            let button = args["button"].as_u64().unwrap_or(1).to_string();
            let steps = (ms / 16).max(2);
            let mut a: Vec<String> = vec!["mousemove".into(), "--sync".into()];
            a.extend(at(o, from.0, from.1));
            a.extend(["mousedown".into(), button.clone()]);
            xdotool(&a)?;
            for i in 1..=steps {
                let t = i as f64 / steps as f64;
                let mut m: Vec<String> = vec!["mousemove".into()];
                m.extend(at(o, from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
                xdotool(&m)?;
                std::thread::sleep(Duration::from_millis(ms / steps));
            }
            xdotool(&["mouseup".into(), button])?;
            then_picture(args, format!("dragged from {from:?} to {to:?} in {ms} ms"))
        }
        _ => Err(format!("no tool '{name}'")),
    }
}

fn tools() -> Value {
    let xy = json!({ "type": "number", "description": "in the window's pixels, as in its screenshot" });
    let pair = json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, y] in the window's pixels" });
    let shot = json!({ "type": "boolean", "description": "a screenshot after it (settle_ms later, default 300)" });
    let settle = json!({ "type": "integer" });
    json!([
        { "name": "start", "description": "Start Hythe (the Surface Duo's desktop app) with its control socket and trace log; restart: true closes a running one first (needed if it was started without the socket); intro: false skips the start animation.",
          "inputSchema": { "type": "object", "properties": { "restart": { "type": "boolean" }, "intro": { "type": "boolean" } } } },
        { "name": "screenshot", "description": "A picture of Hythe's window as it is now (its own pixels; input coordinates are these).",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "state", "description": "Hythe's state: the window's place, the start animation, the floor's squares (size, offset), the drawn Duo (fold, turn), the phone (where it is seen).",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "replay", "description": "Play Hythe's start animation again.",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "click", "description": "Move the real pointer to (x, y) in Hythe's window and click (button 1 left, 2 middle, 3 right; double).",
          "inputSchema": { "type": "object", "properties": { "x": xy, "y": xy, "button": { "type": "integer" }, "double": { "type": "boolean" }, "screenshot": shot, "settle_ms": settle }, "required": ["x", "y"] } },
        { "name": "scroll", "description": "Turn the real mouse wheel over (x, y) in Hythe's window: clicks notches, positive down, negative up.",
          "inputSchema": { "type": "object", "properties": { "x": xy, "y": xy, "clicks": { "type": "integer" }, "delay_ms": { "type": "integer" }, "screenshot": shot, "settle_ms": settle }, "required": ["x", "y", "clicks"] } },
        { "name": "drag", "description": "Press a real mouse button at `from` in Hythe's window, move to `to` over ms milliseconds, release.",
          "inputSchema": { "type": "object", "properties": { "from": pair, "to": pair, "ms": { "type": "integer" }, "button": { "type": "integer" }, "screenshot": shot, "settle_ms": settle }, "required": ["from", "to"] } },
        { "name": "wait", "description": "Wait ms milliseconds (at most 30 s), then a screenshot if asked.",
          "inputSchema": { "type": "object", "properties": { "ms": { "type": "integer" }, "screenshot": { "type": "boolean" } } } },
        { "name": "trace", "description": "The last lines of Hythe's trace log (what it hears from the phone and does).",
          "inputSchema": { "type": "object", "properties": { "lines": { "type": "integer" } } } }
    ])
}

fn result(answer: Result<Answer, String>) -> Value {
    match answer {
        Ok(a) => {
            let mut content = vec![json!({ "type": "text", "text": a.text })];
            if let Some(path) = a.picture {
                match std::fs::read(&path) {
                    Ok(png) => content.push(json!({ "type": "image", "data": base64(&png), "mimeType": "image/png" })),
                    Err(e) => content.push(json!({ "type": "text", "text": format!("{}: {e}", path.display()) })),
                }
            }
            json!({ "content": content })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("call") {
        let name = args.get(2).cloned().unwrap_or_default();
        let a: Value = args.get(3).and_then(|s| serde_json::from_str(s).ok()).unwrap_or(json!({}));
        match call(&name, &a) {
            Ok(a) => {
                println!("{}", a.text);
                if let Some(p) = a.picture {
                    println!("{}", p.display());
                }
            }
            Err(e) => {
                eprintln!("hythe-mcp: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue };
        let method = msg["method"].as_str().unwrap_or_default();
        let reply = match method {
            "initialize" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "hythe", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Hythe's window (the Surface Duo's desktop app): start it, look at it (screenshot, state, trace) and try it with the real pointer (click, scroll, drag) in the window's own pixel coordinates. Input moves the owner's pointer."
            }}),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools() } }),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or_default();
                let args = msg["params"]["arguments"].clone();
                json!({ "jsonrpc": "2.0", "id": id, "result": result(call(name, &args)) })
            }
            _ => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("no method '{method}'") } }),
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}
