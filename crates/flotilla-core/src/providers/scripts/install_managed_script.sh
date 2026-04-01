set -eu

target=$1
content=$2
parent=$(dirname "$target")
temp="$parent/.install-script.$$"

mkdir -p "$parent"
trap 'rm -f "$temp"' EXIT
printf '%s' "$content" > "$temp"
chmod +x "$temp"
mv "$temp" "$target"
trap - EXIT
