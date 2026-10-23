//! Keys for the services the phone speaks to, into its keyring - and never
//! anywhere else. A key goes from this computer to the phone over the same
//! ssh link every command takes (the cable, or the home network with the
//! key the cable showed), inside the script on ssh's standard input, as a
//! here-document fed to `item-secret set NAME` on the phone: not in any
//! process's arguments, not in a file of the phone's, not in a log. On the
//! phone it lives in gnome-keyring's login keyring (encrypted on disk,
//! opened by the PIN at unlock), under item's own schema, read by item
//! (the voice) and by the agent (`item-secret get NAME`). item/grid keeps
//! no copy here and logs only names.
//!
//! The phone must be unlocked for the keyring to take a key.

const HERE: &str = "ITEMGRID_KEY_END";

/// A name as the keyring and the shell both take it: letters, digits, _ and -.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("{name}: a key's name is letters, digits, _ and - (openrouter, spotify_client_id)"));
    }
    Ok(())
}

fn check_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\n') || value.contains('\r') || value.trim() == HERE {
        return Err("a key is one line".into());
    }
    Ok(())
}

/// A script that runs `item-secret ARGS` as the phone's owner, with `input`
/// on its standard input through a here-document in the script itself.
fn owner_script(args: &str, input: Option<&str>) -> String {
    let user = crate::phone::OWNER;
    let run = format!("exec sudo -H -u \"$U\" env XDG_RUNTIME_DIR=/run/user/$I DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$I/bus item-secret {args}");
    match input {
        Some(v) => format!("{user}\ncd \"$H\" 2>/dev/null\n{run} <<'{HERE}'\n{v}\n{HERE}\n"),
        None => format!("{user}\ncd \"$H\" 2>/dev/null\n{run}\n"),
    }
}

/// What a key's name says: the service and what the phone does with it
/// (the Keys page's subtitles). The names are the owner's; these are the
/// ones item/grid knows.
pub fn about(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name.to_ascii_lowercase().replace('-', "_").as_str() {
        "openrouter" => ("OpenRouter", "The keyboard's voice (transcription) and any model through one key; the agent's heavy work."),
        "claude" | "anthropic" => ("Anthropic API", "Scripts that call Claude directly. Claude Code on the phone signs in with your account, not with this."),
        "openai" => ("OpenAI", "Scripts that call OpenAI directly: Whisper, TTS, GPT, when not through OpenRouter."),
        "elevenlabs" => ("ElevenLabs", "A voice for the agent's answers (TTS); Scribe transcription directly."),
        "runpod" => ("RunPod", "A GPU machine raised from the phone for work too heavy for it or for hive."),
        "spotify_client_id" => ("Spotify, client id", "The Web API by OAuth PKCE: no secret needed; the login leaves a refresh token (spotify_refresh_token)."),
        "spotify_refresh_token" => ("Spotify, refresh token", "From the PKCE login: the agent plays and queues music without logging in again."),
        "atlas" => ("World Labs (Marble)", "The World API: a photo, a panorama or a video into a walkable 3D world (Gaussian splats), shown in Chromium."),
        "jev" => ("TypeSafe Jev", "Typed decisions in milliseconds (a choice, a score): a voice phrase routed to the shell, a field or the agent."),
        "subquadratic" | "subq" => ("Subquadratic (SubQ)", "subq-preview, a context of millions of tokens: a whole repository or a day's transcripts in one question."),
        "github" => ("GitHub", "Repositories, issues and pull requests from the phone (gh)."),
        "huggingface" | "hf" => ("Hugging Face", "Models and datasets the agent downloads."),
        "groq" => ("Groq", "Fast open models directly."),
        "replicate" => ("Replicate", "Hosted models by the call."),
        "tavily" => ("Tavily", "Web search for the agent."),
        _ => return None,
    })
}

/// `value` into the phone's keyring under `name`.
pub fn set(host: &str, name: &str, value: &str) -> Result<(), String> {
    check_name(name)?;
    check_value(value)?;
    let out = crate::phone::run_checked(host, &owner_script(&format!("set {name}"), Some(value.trim())))?;
    if !out.contains("kept") {
        return Err(format!("{name}: the phone said: {}", out.trim()));
    }
    Ok(())
}

/// The key under `name`, from the phone's keyring (over the link, in memory).
pub fn get(host: &str, name: &str) -> Result<String, String> {
    check_name(name)?;
    let out = crate::phone::run_checked(host, &owner_script(&format!("get {name}"), None))?;
    let value = out.trim().to_owned();
    if value.is_empty() {
        return Err(format!("{name}: nothing kept"));
    }
    Ok(value)
}

/// Every key on the phone into the file at `path`, `NAME=VALUE` a line,
/// readable by this user alone (0600) - for moving to another phone or a
/// fresh install (the owner's choice: the file holds the keys in the
/// clear). Returns the names written.
pub fn export(host: &str, path: &std::path::Path) -> Result<Vec<String>, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let names = list(host)?;
    let mut lines = String::new();
    for name in &names {
        let value = get(host, name)?;
        lines.push_str(&format!("{name}={value}\n"));
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(lines.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(names)
}

/// The names of the keys on the phone.
pub fn list(host: &str) -> Result<Vec<String>, String> {
    let out = crate::phone::run_checked(host, &owner_script("list", None))?;
    Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

/// The key under `name` forgotten on the phone; whether there was one.
pub fn forget(host: &str, name: &str) -> Result<bool, String> {
    check_name(name)?;
    let out = crate::phone::run_checked(host, &owner_script(&format!("forget {name}"), None))?;
    Ok(out.contains("forgotten"))
}

/// Keys from a file (keys.txt, a .env), a line each: `NAME=VALUE`,
/// `NAME: VALUE` or `NAME VALUE`, an `export ` before the name and quotes
/// around the value allowed; `#` lines and empty ones skipped. Names are
/// lowercased, `-` to `_` (OPENROUTER_API_KEY and openrouter-api-key are
/// one key). Returns the names put on, in the file's order.
pub fn set_all(host: &str, text: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Some((name, value)) = parse_line(line) else {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            return Err(format!("line {}: NAME=VALUE expected", i + 1));
        };
        set(host, &name, &value)?;
        names.push(name);
    }
    Ok(names)
}

/// One line of a keys file: the name (lowercased, `-` to `_`) and the value
/// (quotes taken off); None for a comment, an empty line or no value.
fn parse_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
    let (name, value) = line.split_once(['=', ':']).or_else(|| line.split_once(char::is_whitespace))?;
    let name = name.trim().to_ascii_lowercase().replace('-', "_");
    let value = value.trim();
    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\''))).unwrap_or(value).trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name, value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_goes_in_the_script_not_the_command() {
        let s = owner_script("set openrouter", Some("sk-abc"));
        assert!(s.contains("item-secret set openrouter <<'ITEMGRID_KEY_END'\nsk-abc\nITEMGRID_KEY_END\n"));
        assert!(!s.lines().any(|l| l.contains("sudo") && l.contains("sk-abc")));
    }

    #[test]
    fn the_known_names_are_told() {
        assert_eq!(about("openrouter").map(|a| a.0), Some("OpenRouter"));
        assert_eq!(about("Spotify-Client-ID").map(|a| a.0), Some("Spotify, client id"));
        assert!(about("something_else").is_none());
    }

    #[test]
    fn a_keys_file_line_is_read_in_its_shapes() {
        use super::parse_line;
        assert_eq!(parse_line("openrouter=sk-or-1"), Some(("openrouter".into(), "sk-or-1".into())));
        assert_eq!(parse_line("export OPENROUTER_API_KEY=\"sk-or-1\""), Some(("openrouter_api_key".into(), "sk-or-1".into())));
        assert_eq!(parse_line("Spotify-Client-Id: 'abc'"), Some(("spotify_client_id".into(), "abc".into())));
        assert_eq!(parse_line("atlas  wl-123"), Some(("atlas".into(), "wl-123".into())));
        assert_eq!(parse_line("# a comment"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("alone"), None);
        assert_eq!(parse_line("empty="), None);
    }

    #[test]
    fn names_are_checked() {
        assert!(check_name("openrouter").is_ok());
        assert!(check_name("spotify_client_id").is_ok());
        assert!(check_name("a b").is_err());
        assert!(check_name("").is_err());
    }
}
