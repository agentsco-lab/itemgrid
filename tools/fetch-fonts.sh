#!/bin/sh
# The fonts item/grid's table can set its letters in (the layout editor,
# F2), from Google Fonts (all OFL), their light weight (300), into
# ~/.local/share/fonts/itemgrid. Run once; again does nothing new.
set -e
dir="$HOME/.local/share/fonts/itemgrid"
mkdir -p "$dir"
for family in "Inter" "Roboto" "Open Sans" "Montserrat" "Poppins" "Raleway" "Lato" "Nunito" "Work Sans" "Fira Sans" \
              "IBM Plex Sans" "JetBrains Mono" "Space Grotesk" "Manrope" "Rubik" "Quicksand" "Comfortaa" "Josefin Sans" \
              "Playfair Display" "Merriweather"; do
    file="$dir/$(echo "$family" | tr -d ' ')-Light.ttf"
    [ -s "$file" ] && continue
    q=$(echo "$family" | tr ' ' '+')
    # The stylesheet names the TTF (curl's plain user agent is given TTF).
    # A family with no light weight (Playfair Display) gives its regular.
    url=$(curl -fsS "https://fonts.googleapis.com/css2?family=$q:wght@300" 2>/dev/null | grep -o 'https://[^)]*\.ttf' | head -1)
    [ -n "$url" ] || url=$(curl -fsS "https://fonts.googleapis.com/css2?family=$q:wght@400" | grep -o 'https://[^)]*\.ttf' | head -1)
    if [ -z "$url" ]; then
        echo "fetch-fonts: no light $family" >&2
        continue
    fi
    curl -fsSL "$url" -o "$file"
    echo "fetch-fonts: $family"
done
fc-cache -f "$dir" >/dev/null
