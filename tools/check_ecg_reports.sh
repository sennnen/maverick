#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.." || exit 1

for command in pdfinfo pdftotext pdftoppm shasum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "check_ecg_reports: missing required command: $command" >&2
        exit 1
    fi
done

render_dir=$(mktemp -d "${TMPDIR:-/tmp}/mav-ecg-report-audit.XXXXXX")
cleanup() {
    case "$render_dir" in
        "${TMPDIR:-/tmp}"/mav-ecg-report-audit.*) rm -rf -- "$render_dir" ;;
    esac
}
trap cleanup EXIT

fixtures=(
    n_regular_55
    n_regular_72
    n_regular_90
    a_irregular_70
    a_irregular_90
    a_irregular_110
    o_tachy_120
    o_brady_40
    o_bigeminy_80
)
runtimes=(coreml tflite)

failures=0
fail() {
    echo "check_ecg_reports: $1" >&2
    failures=$((failures + 1))
}

for runtime in "${runtimes[@]}"; do
    for fixture in "${fixtures[@]}"; do
        report="artifacts/ecg-reports/$runtime/${fixture}_${runtime}.pdf"
        if [ ! -f "$report" ]; then
            fail "missing $report"
            continue
        fi

        pages=$(pdfinfo "$report" | awk '/^Pages:/ { print $2 }')
        [ "$pages" = "1" ] || fail "$report has $pages pages; expected 1"

        text_file="$render_dir/${fixture}_${runtime}.txt"
        pdftotext -layout "$report" "$text_file"
        compact=$(tr -d '[:space:]' <"$text_file")
        lower=$(tr '[:upper:]' '[:lower:]' <"$text_file")

        case "$compact" in
            *MAVERICK*) ;;
            *) fail "$report is missing Maverick identity" ;;
        esac
        case "$compact" in
            *Provisionalon-deviceinterpretation*) ;;
            *) fail "$report is missing the provisional label" ;;
        esac
        case "$compact" in
            *30seconds*7680samples*) ;;
            *) fail "$report is missing the exact-recording contract" ;;
        esac
        case "$compact" in
            *25mm/s*10mm/mV* | *25mm/s*sharedrelativegain*) ;;
            *) fail "$report is missing the calibrated-time trace contract" ;;
        esac
        case "$compact" in
            *Thisresearch-onlysoftwareresultisnotadiagnosis*) ;;
            *) fail "$report is missing the safety statement" ;;
        esac
        if printf '%s' "$lower" | grep -Eq 'geminiman|galaxy watch|apkmirror|on-watch'; then
            fail "$report contains forbidden recovered-product branding"
        fi

        pdftoppm -png -r 72 "$report" "$render_dir/${fixture}_${runtime}" >/dev/null 2>&1 ||
            fail "$report did not render through Poppler"
    done
done

rendered=$(find "$render_dir" -name '*.png' -type f | wc -l | tr -d ' ')
[ "$rendered" = "18" ] || fail "rendered $rendered pages; expected 18"

if ! shasum -a 256 -c artifacts/ecg-reports/SHA256SUMS >/dev/null; then
    fail "artifact checksums do not match"
fi

if [ "$failures" -gt 0 ]; then
    echo "check_ecg_reports: $failures problem(s)" >&2
    exit 1
fi

echo "check_ecg_reports: 18 one-page reports, all contracts ok"
