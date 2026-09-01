#!/bin/sh
set -eu

root=$(mktemp -d)
trap 'rm -rf "$root"' 0
fake_bin=$root/bin
install_dir=$root/install
curl_log=$root/curl.log
cosign_log=$root/cosign.log
mkdir -p "$fake_bin" "$install_dir"

cat >"$fake_bin/curl" <<'EOF'
#!/bin/sh
set -eu
output=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output)
      output=$2
      shift 2
      ;;
    http://*|https://*)
      url=$1
      shift
      ;;
    *)
      shift
      ;;
  esac
done
printf '%s\n' "$url" >>"$TEST_CURL_LOG"
printf 'fixture\n' >"$output"
EOF

cat >"$fake_bin/cosign" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >"$TEST_COSIGN_LOG"
exit 1
EOF

chmod 0755 "$fake_bin/curl" "$fake_bin/cosign"
if PATH="$fake_bin:$PATH" \
  TEST_CURL_LOG="$curl_log" \
  TEST_COSIGN_LOG="$cosign_log" \
  POSTGRESEM_INSTALL_DIR="$install_dir" \
  scripts/install.sh v0.3.0-beta.1 >/dev/null 2>&1
then
  echo "installer accepted a failed Sigstore verification" >&2
  exit 1
fi

if grep -q '\.tar\.gz$' "$curl_log"; then
  echo "installer downloaded an archive before authenticating checksums" >&2
  exit 1
fi
grep -qx -- '--certificate-identity' "$cosign_log"
grep -qx -- \
  'https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v0.3.0-beta.1' \
  "$cosign_log"
if [ -e "$install_dir/postgresem" ]; then
  echo "installer wrote a binary after failed Sigstore verification" >&2
  exit 1
fi

echo "installer signature failure checks passed"
