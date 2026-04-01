set -eu

target=$1
content=$2
temp_suffix=$3
parent=$(dirname "$target")

if [ -e "$target" ]; then
  cat "$target"
  exit 0
fi

mkdir -p "$parent"
temp="$parent/.ensure-file.$temp_suffix"
printf '%s' "$content" > "$temp"

if ln "$temp" "$target" 2>/dev/null; then
  cat "$temp"
elif [ -e "$target" ]; then
  cat "$target"
else
  rm -f "$temp"
  exit 1
fi

rm -f "$temp"
