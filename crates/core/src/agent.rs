//! The agent's settings on the phone, set from here (tracker #166, #167,
//! #138): its OpenRouter key and its choices.
//!
//! The key is checked from this computer (OpenRouter's /key: whether it
//! works, its credit) and put into the phone's Secret Service - the owner's
//! keyring, unlocked with their session - through the pinned ssh link, on
//! the shell's standard input: never on a command line, never in a file
//! here, never logged. Afterwards only its last four characters are read
//! back. The choices (model, monthly limit) are a plain file,
//! ~/.config/item/agent, which item follows as it changes.

use serde::Deserialize;

/// The key's place in the keyring: these attributes.
const ATTRS: &str = "service item.agent name openrouter";

/// What OpenRouter says of a key.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub label: String,
    /// Dollars used, and the key's limit if it has one.
    pub usage: f64,
    pub limit: Option<f64>,
}

impl KeyInfo {
    /// In a few words: "$3.20 used of $10.00", "$3.20 used".
    pub fn words(&self) -> String {
        match self.limit {
            Some(l) => format!("${:.2} used of ${l:.2}", self.usage),
            None => format!("${:.2} used", self.usage),
        }
    }
}

#[derive(Deserialize)]
struct KeyReply {
    data: KeyData,
}

#[derive(Deserialize)]
struct KeyData {
    #[serde(default)]
    label: String,
    #[serde(default)]
    usage: f64,
    limit: Option<f64>,
}

/// Whether `key` works, asked of OpenRouter from this computer.
pub fn check(key: &str) -> Result<KeyInfo, String> {
    let key = key.trim();
    if !key.starts_with("sk-or-") || key.len() < 20 || key.chars().any(char::is_whitespace) {
        return Err("that does not look like an OpenRouter key (sk-or-...)".into());
    }
    let agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(15))).http_status_as_error(false).build().new_agent();
    let mut reply = agent.get("https://openrouter.ai/api/v1/key").header("Authorization", &format!("Bearer {key}")).call().map_err(|e| format!("OpenRouter did not answer: {e}"))?;
    match reply.status().as_u16() {
        200 => {}
        401 | 403 => return Err("OpenRouter does not know this key".into()),
        s => return Err(format!("OpenRouter answered {s}")),
    }
    let r: KeyReply = reply.body_mut().read_json().map_err(|e| format!("OpenRouter's answer: {e}"))?;
    Ok(KeyInfo { label: r.data.label, usage: r.data.usage, limit: r.data.limit })
}

/// The key put into the phone's keyring (replacing one there).
pub fn store(host: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.contains('\'') || key.contains('\n') {
        return Err("the key has characters a key does not".into());
    }
    // As root: the key into a file only the owner can read, in their
    // runtime directory (memory), by printf - the shell's own, so the key is
    // on no command line; then as the owner secret-tool reads it from there
    // and the file goes. sudo's command line (which it logs) holds only the
    // file's name.
    let script = format!(
        r#"{user}
umask 077
f=$(mktemp -p /run/user/$I .hythe-key.XXXXXX) || exit 1
printf '%s' '{key}' > "$f"
chown "$U" "$f"
sudo -u "$U" env XDG_RUNTIME_DIR=/run/user/$I DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$I/bus sh -c 'secret-tool store --label="item: OpenRouter key" {ATTRS} < "$0" && echo stored; rm -f "$0"' "$f"
rm -f "$f"
"#,
        user = crate::phone::OWNER
    );
    let out = crate::phone::run(host, &script)?;
    if out.trim() == "stored" {
        Ok(())
    } else {
        Err("the phone's keyring did not take it (is the phone unlocked since its start?)".into())
    }
}

/// The key on the phone, if one is: its last four characters only.
pub fn stored(host: &str) -> Result<Option<String>, String> {
    let out = crate::phone::run(host, &crate::phone::as_owner(&format!("secret-tool lookup {ATTRS} 2>/dev/null | tail -c 4; echo\n")))?;
    let last = out.trim();
    Ok((!last.is_empty()).then(|| last.to_owned()))
}

/// The key taken out of the phone's keyring.
pub fn forget(host: &str) -> Result<(), String> {
    crate::phone::run(host, &crate::phone::as_owner(&format!("secret-tool clear {ATTRS}\n"))).map(|_| ())
}

/// The agent's choices on the phone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Choices {
    /// OpenRouter's model id; empty: item's default.
    pub model: String,
    /// Dollars a month before the agent asks; None: no limit.
    pub monthly_limit: Option<f64>,
}

/// The choices as the phone has them (~/.config/item/agent).
pub fn choices(host: &str) -> Result<Choices, String> {
    let text = crate::phone::run(host, &crate::phone::as_owner("cat \"$HOME/.config/item/agent\" 2>/dev/null\n"))?;
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(str::trim).unwrap_or_default().to_owned();
    Ok(Choices { model: get("model"), monthly_limit: get("monthly_limit").parse().ok() })
}

/// The choices written on the phone.
pub fn set_choices(host: &str, c: &Choices) -> Result<(), String> {
    if c.model.contains(['\'', '\n', '"', '$', '`', '\\']) {
        return Err("a model id has no such characters".into());
    }
    let limit = c.monthly_limit.map(|l| format!("{l:.2}")).unwrap_or_default();
    let script = format!("mkdir -p \"$HOME/.config/item\" && printf 'model=%s\\nmonthly_limit=%s\\n' '{}' '{limit}' > \"$HOME/.config/item/agent.new\" && mv \"$HOME/.config/item/agent.new\" \"$HOME/.config/item/agent\" && echo ok\n", c.model);
    let out = crate::phone::run(host, &crate::phone::as_owner(&script))?;
    if out.trim() == "ok" {
        Ok(())
    } else {
        Err("the phone did not keep the agent's settings".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_what_is_no_key() {
        assert!(super::check("hello").is_err());
        assert!(super::check("sk-or-v1-short").is_err());
    }
}
