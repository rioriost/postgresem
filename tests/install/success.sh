#!/bin/sh
set -eu

root=$(mktemp -d)
trap 'rm -rf "$root"' 0
fixture_dir=$root/release
fake_bin=$root/bin
version=0.4.0
mkdir -p "$fixture_dir" "$fake_bin"

for asset_arch in amd64 arm64; do
  archive_root="postgresem-${version}-linux-${asset_arch}"
  package_dir=$root/$archive_root
  mkdir -p "$package_dir"
  cat >"$package_dir/postgresem" <<EOF
#!/bin/sh
printf '%s\n' 'postgresem fixture-${asset_arch}'
EOF
  chmod 0755 "$package_dir/postgresem"
  tar -czf "$fixture_dir/${archive_root}.tar.gz" -C "$root" "$archive_root"
done

(
  cd "$fixture_dir"
  : >SHA256SUMS
  for archive in ./*.tar.gz; do
    if command -v shasum >/dev/null 2>&1; then
      checksum=$(shasum -a 256 "$archive" | awk '{ print $1 }')
    else
      checksum=$(sha256sum "$archive" | awk '{ print $1 }')
    fi
    printf '%s  %s\n' "$checksum" "${archive#./}" >>SHA256SUMS
  done
)
printf 'fixture-signature\n' >"$fixture_dir/SHA256SUMS.sig"
printf 'fixture-certificate\n' >"$fixture_dir/SHA256SUMS.pem"

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
cp "$TEST_FIXTURE_DIR/${url##*/}" "$output"
EOF

cat >"$fake_bin/cosign" <<'EOF'
#!/bin/sh
exit 0
EOF

cat >"$fake_bin/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
  -s) printf '%s\n' Linux ;;
  -m) printf '%s\n' "$TEST_UNAME_ARCH" ;;
  *) exit 2 ;;
esac
EOF

chmod 0755 "$fake_bin/curl" "$fake_bin/cosign" "$fake_bin/uname"

for architecture in x86_64:amd64 aarch64:arm64; do
  uname_arch=${architecture%:*}
  asset_arch=${architecture#*:}
  install_dir=$root/install-$asset_arch
  mkdir -p "$install_dir"

  PATH="$fake_bin:$PATH" \
    TEST_FIXTURE_DIR="$fixture_dir" \
    TEST_UNAME_ARCH="$uname_arch" \
    POSTGRESEM_INSTALL_DIR="$install_dir" \
    scripts/install.sh "$version" >/dev/null

  output=$("$install_dir/postgresem" --version)
  if [ "$output" != "postgresem fixture-${asset_arch}" ]; then
    echo "installer selected the wrong Linux architecture" >&2
    exit 1
  fi
done

echo "installer Linux architecture and success checks passed"
