#!/bin/sh
# The demo home of fixture.sh, grown into one that looks lived in, for the
# everyday-task recordings (task-*.tape): seven years of teaching, over a
# thousand files; a cluttered Downloads; a folder of photos; and a few
# large files to find (sparse, so they take no room). Usage:
# fixture-tasks.sh DIR (removed and made again).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
demo=${1:?usage: fixture-tasks.sh DIR}
sh "$here/fixture.sh" "$demo"
home=$demo/home

# Teaching: a folder per academic year, course and week, plus the exams.
for year in 2019 2020 2021 2022 2023 2024 2025; do
  span=$year-$((year + 1))
  for course in calculus linear-algebra real-analysis pde; do
    dir=$home/teaching/$span/$course
    for n in 01 02 03 04 05 06 07 08 09 10 11 12 13 14; do
      mkdir -p "$dir/week-$n"
      printf '\\section*{%s, week %s: exercises}\n' "$course" "$n" \
        > "$dir/week-$n/exercises.tex"
      printf '\\section*{%s, week %s: solutions}\n' "$course" "$n" \
        > "$dir/week-$n/solutions.tex"
      printf 'demo\n' > "$dir/week-$n/slides.pdf"
    done
    for exam in january june; do
      printf '\\section*{%s: %s %s exam}\n' "$course" "$exam" "$span" \
        > "$dir/exam-$exam.tex"
    done
  done
  find "$home/teaching/$span" -exec touch -d "$year-10-01" {} +
done
# Some exam questions to find again.
t=$home/teaching
printf '%s\n' '\item Prove the Cauchy--Schwarz inequality for an inner product space.' \
  >> "$t/2021-2022/linear-algebra/exam-june.tex"
printf '%s\n' '\item Use Cauchy--Schwarz to show that $|\int fg| \le \|f\|_2 \|g\|_2$.' \
  >> "$t/2023-2024/real-analysis/exam-january.tex"
printf '%s\n' '\item State the Cauchy--Schwarz inequality and give an example.' \
  >> "$t/2025-2026/linear-algebra/exam-january.tex"
printf '%s\n' '% Exercise 3 uses Cauchy--Schwarz; see the exam.' \
  >> "$t/2025-2026/linear-algebra/week-05/solutions.tex"
printf '%s\n' '\item Show that every Cauchy sequence in $\mathbb{R}$ converges.' \
  >> "$t/2024-2025/real-analysis/exam-june.tex"

# Downloads: what piles up in a few months, from months ago to yesterday.
dl=$home/Downloads
file() { # file NAME SIZE DATE
  truncate -s "$2" "$dl/$1"
  touch -d "$3" "$dl/$1"
}
file ubuntu-24.04.1-desktop-amd64.iso 6G '2026-03-14'
file zoom_amd64.deb 182M '2026-04-02'
file 2409.11873.pdf 2M '2026-05-20'
file 2410.03021.pdf 1M '2026-06-11'
file 2502.17740.pdf 3M '2026-09-03'
file referee-report-final.pdf 210K '2026-10-21'
file thesis-defense-slides.pdf 14M '2026-07-01'
file IMG_20260712_1830.jpg 4M '2026-07-12'
file Screenshot-2026-09-30.png 380K '2026-09-30'
file Screenshot-2026-10-02.png 410K '2026-10-02'
file grades-calculus.xlsx 64K '2026-07-04'
file lecture-recording.mp4 1400M '2026-10-15'
file dataset-sensors.zip 820M '2026-08-19'
file conference-poster.svg 1M '2026-06-28'
file train-tickets.pdf 90K '2026-11-05'
# fixture.sh's own downloads, made to look as old and as large.
file invoice-2026-10.pdf 120K '2026-10-03'
file boarding-pass.pdf 300K '2026-09-12'
file photo-001.jpg 5M '2026-08-14'
file photo-002.jpg 5M '2026-08-14'
file dataset.csv 12M '2026-06-02'
file slides-draft.odp 2M '2026-10-27'
touch -d '2026-11-05' "$dl"

# Photos to rename, a minute apart.
pics=$home/Pictures/mallorca-2026
mkdir -p "$pics"
n=2031
while [ $n -le 2048 ]; do
  truncate -s 3M "$pics/IMG_$n.jpg"
  touch -d "2026-08-0$((n % 7 + 1)) 12:$((n % 50 + 10))" "$pics/IMG_$n.jpg"
  n=$((n + 1))
done

# Large files hidden in the tree.
mkdir -p "$home/Videos" "$home/backup"
truncate -s 3800M "$home/Videos/defense-rehearsal.mkv"
truncate -s 1200M "$home/Videos/seminar-2024-03.mp4"
truncate -s 9G "$home/backup/laptop-2023.tar"
touch -d 2025-12-18 "$home/Videos/defense-rehearsal.mkv"
touch -d 2024-03-12 "$home/Videos/seminar-2024-03.mp4"
touch -d 2023-06-30 "$home/backup/laptop-2023.tar"
