//! item's look on the phone, set from here (tracker #166): the wallpapers
//! and the accent.
//!
//! As item keeps them: the pictures in ~/.local/share/item/walls (the
//! owner's) and /usr/share/item/walls (the package's), each a .jpg with an
//! optional NAME.txt beside it (its credit) and a small thumb-NAME.jpg item
//! makes; what is on in ~/.config/item/wallpaper ("mode both|each",
//! "both|left|right NAME ZOOM CX CY", "vignette ..."); the accent in
//! ~/.config/item/accent, `auto` (from the wallpaper) or `#rrggbb`.

use std::path::Path;

/// item's accents after "from the wallpaper" (compositor/src/accent.rs).
pub const PALETTE: [(&str, &str); 7] = [
    ("Coral", "#f08a7b"),
    ("Yellow", "#e9df6a"),
    ("Green", "#86d68f"),
    ("Teal", "#62cfc6"),
    ("Blue", "#7daaf7"),
    ("Violet", "#b798f0"),
    ("Pink", "#f092bb"),
];

/// One picture in item's set.
#[derive(Debug, Clone)]
pub struct Wall {
    pub name: String,
    /// Who made it, as its NAME.txt says.
    pub credit: String,
    /// The owner's (removable), not the package's.
    pub own: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Look {
    /// `auto` or `#rrggbb`.
    pub accent: String,
    pub walls: Vec<Wall>,
    /// What is on: the picture on both panels, or the left one's and the
    /// right one's with "each".
    pub each: bool,
    pub both: String,
    pub left: String,
    pub right: String,
}

const OWN: &str = "$HOME/.local/share/item/walls";

/// A wallpaper's name as item takes it: a .jpg, letters, digits, - _ .
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.starts_with("thumb-") && name.ends_with(".jpg") && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// The look as the phone has it.
pub fn read(host: &str) -> Result<Look, String> {
    let script = format!(
        r#"echo "accent=$(cat "$HOME/.config/item/accent" 2>/dev/null)"
for d in "{OWN}" /usr/share/item/walls; do
  for f in "$d"/*.jpg "$d"/*.jpeg; do
    [ -f "$f" ] || continue
    n=${{f##*/}}; case $n in thumb-*) continue;; esac
    c=$(head -c 200 "${{f%.*}}.txt" 2>/dev/null | head -1)
    [ "$d" = "{OWN}" ] && o=1 || o=0
    printf 'wall\t%s\t%s\t%s\n' "$o" "$n" "$c"
  done
done
sed 's/^/kept\t/' "$HOME/.config/item/wallpaper" 2>/dev/null
"#
    );
    let out = crate::phone::run(host, &crate::phone::as_owner(&script))?;
    let mut look = Look::default();
    for line in out.lines() {
        if let Some(a) = line.strip_prefix("accent=") {
            look.accent = if a.trim().is_empty() { "auto".into() } else { a.trim().to_owned() };
        } else if let Some(w) = line.strip_prefix("wall\t") {
            let mut f = w.splitn(3, '\t');
            let (own, name, credit) = (f.next() == Some("1"), f.next().unwrap_or_default(), f.next().unwrap_or_default());
            if !look.walls.iter().any(|x| x.name == name) {
                look.walls.push(Wall { name: name.to_owned(), credit: credit.trim().to_owned(), own });
            }
        } else if let Some(k) = line.strip_prefix("kept\t") {
            let w: Vec<&str> = k.split_whitespace().collect();
            match w.as_slice() {
                ["mode", m] => look.each = *m == "each",
                ["both", n, ..] => look.both = n.to_string(),
                ["left", n, ..] => look.left = n.to_string(),
                ["right", n, ..] => look.right = n.to_string(),
                _ => {}
            }
        }
    }
    look.walls.sort_by(|a, b| b.own.cmp(&a.own).then(a.name.cmp(&b.name)));
    Ok(look)
}

/// A picture's small version for showing here: item's thumb if it made one,
/// else the picture itself.
pub fn thumb(host: &str, name: &str) -> Result<Vec<u8>, String> {
    if !plain(name) && !name.ends_with(".jpeg") {
        return Err("not a wallpaper's name".into());
    }
    let script = format!(
        r#"for d in "{OWN}" /usr/share/item/walls; do
  [ -f "$d/thumb-{name}" ] && exec cat "$d/thumb-{name}"
done
for d in "{OWN}" /usr/share/item/walls; do
  [ -f "$d/{name}" ] && exec cat "$d/{name}"
done
"#
    );
    crate::phone::run_bytes(host, &crate::phone::as_owner(&script))
}

/// The accent chosen: `auto` or `#rrggbb`.
pub fn set_accent(host: &str, accent: &str) -> Result<(), String> {
    let ok = accent == "auto" || (accent.len() == 7 && accent.starts_with('#') && accent[1..].chars().all(|c| c.is_ascii_hexdigit()));
    if !ok {
        return Err("an accent is auto or #rrggbb".into());
    }
    let script = format!("mkdir -p \"$HOME/.config/item\" && printf '%s' '{accent}' > \"$HOME/.config/item/accent.new\" && mv \"$HOME/.config/item/accent.new\" \"$HOME/.config/item/accent\" && echo ok\n");
    expect_ok(crate::phone::run(host, &crate::phone::as_owner(&script))?, "the phone did not keep the accent")
}

/// A picture put on both panels, whole (the vignette and the panels' own
/// pictures kept, for when "each" is chosen on the phone).
pub fn set_wallpaper(host: &str, name: &str) -> Result<(), String> {
    if !plain(name) && !name.ends_with(".jpeg") {
        return Err("not a wallpaper's name".into());
    }
    let script = format!(
        r#"f="$HOME/.config/item/wallpaper"; mkdir -p "$HOME/.config/item"
{{ echo "mode both"; grep -v -e '^mode ' -e '^both ' "$f" 2>/dev/null; echo "both {name} 1.000 0.5000 0.5000"; }} > "$f.new" && mv "$f.new" "$f" && echo ok
"#
    );
    expect_ok(crate::phone::run(host, &crate::phone::as_owner(&script))?, "the phone did not keep the wallpaper")
}

/// A picture from this computer added to the owner's set (a .jpg; the window
/// makes one of any other picture first), with its credit; its name there.
pub fn add_wall(host: &str, jpg: &Path, wanted: &str, credit: &str) -> Result<String, String> {
    let stem: String = wanted.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let stem = stem.trim_matches('-');
    let name = format!("{}.jpg", if stem.is_empty() { "picture" } else { stem });
    if !plain(&name) {
        return Err("not a wallpaper's name".into());
    }
    if credit.contains(['\'', '\n']) {
        return Err("a credit is one line".into());
    }
    let tmp = format!("/var/tmp/hythe-wall-{}.jpg", std::process::id());
    crate::phone::put(host, jpg, &tmp)?;
    // As root: into the owner's folder under a name not taken, theirs.
    let script = format!(
        r#"{user}
H=$(getent passwd "$U" | cut -d: -f6); d="$H/.local/share/item/walls"
sudo -u "$U" mkdir -p "$d"
n='{name}'; b=${{n%.jpg}}; k=2
while [ -e "$d/$n" ]; do n="$b-$k.jpg"; k=$((k+1)); done
mv '{tmp}' "$d/$n" && chown "$U:" "$d/$n" && chmod 644 "$d/$n" || {{ rm -f '{tmp}'; exit 1; }}
[ -n '{credit}' ] && printf '%s\n' '{credit}' > "$d/${{n%.jpg}}.txt" && chown "$U:" "$d/${{n%.jpg}}.txt"
echo "$n"
"#,
        user = crate::phone::OWNER
    );
    let n = crate::phone::run_checked(host, &script)?;
    let n = n.trim().to_owned();
    if plain(&n) {
        Ok(n)
    } else {
        Err("the phone did not take the picture".into())
    }
}

/// One of the owner's pictures taken out of the set, with its thumb and
/// credit (the package's stay).
pub fn remove_wall(host: &str, name: &str) -> Result<(), String> {
    if !plain(name) && !name.ends_with(".jpeg") {
        return Err("not a wallpaper's name".into());
    }
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let script = format!("cd \"{OWN}\" && rm -f -- '{name}' 'thumb-{name}' '{stem}.txt' && echo ok\n");
    expect_ok(crate::phone::run(host, &crate::phone::as_owner(&script))?, "the phone kept the picture")
}

fn expect_ok(out: String, otherwise: &str) -> Result<(), String> {
    if out.trim() == "ok" {
        Ok(())
    } else {
        Err(otherwise.into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert!(super::plain("bruno-guerrero-BP4jt_A0gNo-unsplash.jpg"));
        assert!(!super::plain("thumb-a.jpg"));
        assert!(!super::plain("a b.jpg"));
        assert!(!super::plain("../x.jpg"));
        assert!(!super::plain("x.png"));
    }
}
