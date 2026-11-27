#!/bin/bash
# usage: bench.sh CLIP WIDTH HEIGHT RUNS
# BENCH_DIR holds bench/ (the clips), x-ffmpeg/, x-gifski/ and midi-bench/ (built from midi-bench.rs)
S="${BENCH_DIR:?set BENCH_DIR}"
F="$S/x-ffmpeg/ffmpeg-9.1-essentials_build/bin/ffmpeg.exe"
G="$S/x-gifski/win/gifski.exe"
M="$S/midi-bench/target/release/midi-bench.exe"
clip=$1; w=$2; h=$3; runs=${4:-3}
cd "$S/bench"; mkdir -p out
raw_in="-f rawvideo -pix_fmt rgb24 -s ${w}x${h} -r 15 -i $clip.rgb"

declare -A CMD
CMD[midi-L1]="$M $clip.rgb $w $h 15 1 out/$clip-midi-L1.gif"
CMD[midi-L2]="$M $clip.rgb $w $h 15 2 out/$clip-midi-L2.gif"
CMD[ffmpeg-sharex]="$F -hide_banner -loglevel error -y $raw_in -vf \"split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a\" out/$clip-ffmpeg-sharex.gif"
CMD[ffmpeg-bayer-diff]="$F -hide_banner -loglevel error -y $raw_in -vf \"split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=bayer:diff_mode=rectangle\" out/$clip-ffmpeg-bayer-diff.gif"
CMD[gifski]="$G --quiet --fps 15 --width $w -o out/$clip-gifski.gif png-$clip/*.png"

for name in ${ONLY:-midi-L1 midi-L2 ffmpeg-sharex ffmpeg-bayer-diff gifski}; do
  times=()
  for ((r=0; r<runs; r++)); do
    t0=$(date +%s.%N); eval ${CMD[$name]} > /dev/null; t1=$(date +%s.%N)
    times+=($(awk "BEGIN{print $t1 - $t0}"))
  done
  median=$(printf '%s\n' "${times[@]}" | sort -n | sed -n "$(( (runs+1)/2 ))p")
  gif="out/$clip-$name.gif"
  bytes=$(stat -c %s "$gif")
  q=$("$F" -hide_banner -i "$gif" $raw_in -lavfi "[0:v]fps=15,format=rgb24,split[a1][a2];[1:v]split[b1][b2];[a1][b1]ssim;[a2][b2]psnr" -f null - 2>&1)
  ssim=$(echo "$q" | grep -o 'SSIM .*All:[0-9.]*' | grep -o 'All:[0-9.]*' | cut -d: -f2)
  psnr=$(echo "$q" | grep -o 'PSNR .*average:[0-9.inf]*' | grep -o 'average:[0-9.inf]*' | cut -d: -f2)
  frames=$(echo "$q" | grep -o 'frame= *[0-9]*' | tail -1 | tr -dc 0-9)
  dims=$("$F" -hide_banner -i "$gif" 2>&1 | grep -o 'Video: gif.*' | grep -oE '[0-9]+x[0-9]+' | head -1)
  printf '%s,%s,%s,%.2f,%s,%s,%s,%s,%s\n' "$clip" "$name" "$dims" "$median" "$bytes" "$ssim" "$psnr" "$frames" "${times[*]}"
done
