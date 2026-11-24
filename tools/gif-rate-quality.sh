#!/bin/bash
# Compares midi with gifski at the same file size, the fair way to compare GIF encoders that trade size for quality.
#
# usage: tools/gif-rate-quality.sh [DITHER]
#   GIF_BENCH_DIR  folder holding action.rgb (640x360) and action720.rgb (1280x720) as raw rgb24 at 15fps,
#                  plus png-<clip>/ with the same frames as PNG for gifski
#   FFMPEG, GIFSKI paths to the tools
#
# Encodes each clip with midi at quality levels 1-5 and with gifski at quality 100, 95, 90, 80, 70, 60 (gifski's
# results are cached in GIF_BENCH_DIR/gifski-<clip>.csv). For each midi file it reads gifski's psnr, ssim and
# blurred psnr at the same size off gifski's curve (linear in log size) and prints midi minus gifski. Positive is
# better than gifski. A row outside gifski's size range prints "out".
set -u
dither=${1:-1.0}
root=$(cd "$(dirname "$0")/.." && pwd)
: "${GIF_BENCH_DIR:?set GIF_BENCH_DIR}" "${FFMPEG:?set FFMPEG}" "${GIFSKI:?set GIFSKI}"

cargo build --release --quiet --example encode_raw -p gif-core --manifest-path "$root/Cargo.toml" || exit 1
encoder="$root/target/release/examples/encode_raw"
out="$root/target/gif-quality"; mkdir -p "$out"; cd "$out" || exit 1

psnr_of() { grep -oE 'mse_avg:[0-9.]+' "$1" | cut -d: -f2 | awk '{s+=$1;n++} END {printf "%.3f", 10*log(65025/(s/n))/log(10)}'; }
measure() {
  local gif=$1 raw=$2 w=$3 h=$4 ssim
  local source="-f rawvideo -pix_fmt rgb24 -s ${w}x${h} -r 15 -i $raw"
  rm -f p.txt b.txt
  ssim=$("$FFMPEG" -hide_banner -i "$gif" $source -lavfi \
    "[0:v]fps=15,format=rgb24,split[a1][a2];[1:v]split[b1][b2];[a1][b1]psnr=stats_file=p.txt;[a2][b2]ssim" \
    -f null - 2>&1 | grep -oE 'All:[0-9.]+' | tail -1 | cut -d: -f2)
  "$FFMPEG" -hide_banner -loglevel error -i "$gif" $source -lavfi \
    "[0:v]fps=15,format=rgb24,gblur=sigma=1.5[a];[1:v]gblur=sigma=1.5[b];[a][b]psnr=stats_file=b.txt" -f null -
  echo "$(stat -c %s "$gif"),$(psnr_of p.txt),$ssim,$(psnr_of b.txt)"
}

echo "clip,level,bytes,psnr,ssim,blur_psnr,d_psnr,d_ssim,d_blur_psnr"
for spec in "action 640 360" "action720 1280 720"; do
  set -- $spec; clip=$1; w=$2; h=$3
  raw=$(cygpath -m "$GIF_BENCH_DIR/$clip.rgb")
  curve="$GIF_BENCH_DIR/gifski-$clip.csv"
  if [ ! -s "$curve" ]; then
    for quality in 100 95 90 80 70 60; do
      "$GIFSKI" --quiet --fps 15 --width "$w" --quality "$quality" -o gifski.gif "$GIF_BENCH_DIR/png-$clip"/*.png
      echo "$quality,$(measure gifski.gif "$raw" "$w" "$h")" >> "$curve"
    done
  fi
  for level in 1 2 3 4 5; do
    "$encoder" "$raw" "$w" "$h" 15 "$level" midi.gif "$dither" > /dev/null || { echo "$clip,$level,error"; continue; }
    row=$(measure midi.gif "$raw" "$w" "$h")
    awk -F, -v clip="$clip" -v level="$level" -v row="$row" '
      { b[NR] = log($2); p[NR] = $3; s[NR] = $4; z[NR] = $5; n = NR }
      END {
        split(row, m, ","); x = log(m[1]); found = 0
        for (i = 1; i < n; i++) {
          hi = (b[i] > b[i+1]) ? i : i + 1; lo = (hi == i) ? i + 1 : i
          if (x <= b[hi] && x >= b[lo]) {
            t = (x - b[lo]) / (b[hi] - b[lo]); found = 1
            printf "%s,%s,%s,%s,%s,%s,%+.3f,%+.4f,%+.3f\n", clip, level, m[1], m[2], m[3], m[4],
              m[2] - (p[lo] + t * (p[hi] - p[lo])), m[3] - (s[lo] + t * (s[hi] - s[lo])), m[4] - (z[lo] + t * (z[hi] - z[lo]))
            break
          }
        }
        if (!found) printf "%s,%s,%s,%s,%s,%s,out,out,out\n", clip, level, m[1], m[2], m[3], m[4]
      }' "$curve"
  done
done
