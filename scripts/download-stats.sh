#!/usr/bin/env bash
set -euo pipefail

# Download Stats — Parse CloudFront access logs for download counts
# Usage: download-stats.sh [VERSION]
#   VERSION: optional filter, e.g. "v0.17.7"
#
# CloudFront log fields (tab-separated):
#   $1=date $6=cs-method $8=cs-uri-stem $9=sc-status

AWS_PROFILE="${AWS_PROFILE:-logs-reader}"
LOG_BUCKET="s3://unfudged-cf-logs/downloads/"
CACHE_DIR=".download-logs"
VERSION_FILTER="${1:-}"

# Sync logs from S3
mkdir -p "$CACHE_DIR"
echo "Syncing CloudFront logs..."
aws s3 sync "$LOG_BUCKET" "$CACHE_DIR/" --profile "$AWS_PROFILE" --quiet
echo ""

# Check for log files
if ! ls "$CACHE_DIR"/*.gz &>/dev/null; then
  echo "No log files found."
  exit 0
fi

# Extract: date, version, platform from successful GET requests to /releases/
PARSED=$(gunzip -c "$CACHE_DIR"/*.gz 2>/dev/null \
  | awk -F'\t' -v ver_filter="$VERSION_FILTER" '
/^#/ { next }
$6 == "GET" && $9 == "200" && $8 ~ /^\/releases\// {
  uri = $8
  date = $1

  # Extract version: /releases/v0.17.7/... -> v0.17.7
  ver = uri
  sub(/^\/releases\//, "", ver)
  sub(/\/.*/, "", ver)

  # Detect platform
  if (uri ~ /aarch64-apple-darwin/) plat = "macOS ARM"
  else if (uri ~ /x86_64-apple-darwin/) plat = "macOS Intel"
  else if (uri ~ /x86_64-unknown-linux/) plat = "Linux"
  else if (uri ~ /universal\.dmg/) plat = "Desktop (.dmg)"
  else plat = "Other"

  # Apply version filter
  if (ver_filter != "" && ver != ver_filter) next

  print date "\t" ver "\t" plat
}')

TOTAL=$(echo "$PARSED" | grep -c . 2>/dev/null || echo 0)

# Header
if [ -n "$VERSION_FILTER" ]; then
  echo "Download Report — $VERSION_FILTER"
else
  echo "Download Report"
fi
printf '=%.0s' {1..40}
echo ""
echo ""
echo "Total Downloads: $TOTAL"

if [ "$TOTAL" -eq 0 ]; then
  exit 0
fi

echo ""

# By Version
echo "By Version:"
echo "$PARSED" | awk -F'\t' '{print $2}' | sort | uniq -c | sort -rn | while read -r count ver; do
  pct=$(awk "BEGIN { printf \"%.1f\", ($count / $TOTAL) * 100 }")
  printf "  %-14s %4d  (%5.1f%%)\n" "$ver" "$count" "$pct"
done

echo ""

# By Platform
echo "By Platform:"
echo "$PARSED" | awk -F'\t' '{print $3}' | sort | uniq -c | sort -rn | while read -r count plat; do
  pct=$(awk "BEGIN { printf \"%.1f\", ($count / $TOTAL) * 100 }")
  printf "  %-14s %4d  (%5.1f%%)\n" "$plat" "$count" "$pct"
done

echo ""

# By Day (most recent first)
echo "By Day:"
echo "$PARSED" | awk -F'\t' '{print $1}' | sort -r | uniq -c | awk '{printf "  %s  %4d\n", $2, $1}'
