#!/bin/bash
# Measures midi's GIF encoder on real video clips: time, size and how close the GIF is to the source.
#
# usage: tools/gif-quality.sh [RUNS] [DITHER]
#   GIF_BENCH_DIR  folder holding action.rgb (640x360), snow.rgb (640x360) and action720.rgb (1280x720),
#                  raw rgb24 frames at 15fps
#   FFMPEG         path to ffmpeg
#
# Prints one CSV row per clip:
#   clip,frames_expected,frames_decoded,errors,median_s,min_s,max_s,bytes,psnr,ssim,blur_psnr
# psnr and ssim compare each frame as it is. blur_psnr compares both after a small blur, which is roughly
# how dithering looks from viewing distance, so it drops when gradients band.
set -u
runs=${1:-5}
dither=${2:-1.0}
root=$(cd "$(dirname "$0")/.." && pwd)
: "${GIF_BENCH_DIR:?set GIF_BENCH_DIR}" "${FFMPEG:?set FFMPEG}"

cargo build --release --quiet --example encode_raw -p gif-core --manifest-path "$root/Cargo.toml" || exit 1
encoder="$root/target/release/examples/encode_raw"
out="$root/target/gif-quality"; mkdir -p "$out"; cd "$out" || exit 1

psnr_of() { grep -oE 'mse_avg:[0-9.]+' "$1" | cut -d: -f2 | awk '{s+=$1;n++} END {printf "%.3f", 10*log(65025/(s/n))/log(10)}'; }

echo "clip,frames_expected,frames_decoded,errors,median_s,min_s,max_s,bytes,psnr,ssim,blur_psnr"
for spec in "action 640 360" "snow 640 360" "action720 1280 720"; do
  set -- $spec; clip=$1; w=$2; h=$3
  raw=$(cygpath -m "$GIF_BENCH_DIR/$clip.rgb")
  expected=$(( $(stat -c %s "$raw") / (w * h * 3) ))
  gif="$clip.gif"
  rm -f "$gif" p.txt b.txt
  errors=0; times=()
  for ((r = 0; r < runs; r++)); do
    t0=$(date +%s.%N)
    "$encoder" "$raw" "$w" "$h" 15 1 "$gif" "$dither" > /dev/null 2>&1 || errors=$((errors + 1))
    t1=$(date +%s.%N)
    times+=("$(awk "BEGIN{print $t1 - $t0}")")
  done
  sorted=$(printf '%s\n' "${times[@]}" | sort -n)
  median=$(echo "$sorted" | sed -n "$(( (runs + 1) / 2 ))p"); min=$(echo "$sorted" | head -1); max=$(echo "$sorted" | tail -1)
  source="-f rawvideo -pix_fmt rgb24 -s ${w}x${h} -r 15 -i $raw"
  ssim=$("$FFMPEG" -hide_banner -i "$gif" $source -lavfi \
    "[0:v]fps=15,format=rgb24,split[a1][a2];[1:v]split[b1][b2];[a1][b1]psnr=stats_file=p.txt;[a2][b2]ssim" \
    -f null - 2>&1 | grep -oE 'All:[0-9.]+' | tail -1 | cut -d: -f2)
  "$FFMPEG" -hide_banner -loglevel error -i "$gif" $source -lavfi \
    "[0:v]fps=15,format=rgb24,gblur=sigma=1.5[a];[1:v]gblur=sigma=1.5[b];[a][b]psnr=stats_file=b.txt" -f null -
  decoded=$(wc -l < p.txt)
  [ "$decoded" -eq "$expected" ] || errors=$((errors + 1))
  printf '%s,%s,%s,%s,%.3f,%.3f,%.3f,%s,%s,%s,%s\n' "$clip" "$expected" "$decoded" "$errors" "$median" "$min" "$max" \
    "$(stat -c %s "$gif")" "$(psnr_of p.txt)" "$ssim" "$(psnr_of b.txt)"
done

