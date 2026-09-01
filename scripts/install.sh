#!/bin/sh
set -eu

program=postgresem
repository=${POSTGRESEM_REPOSITORY:-rioriost/postgresem}
install_dir=${POSTGRESEM_INSTALL_DIR:-"$HOME/.local/bin"}
requested_version=${1:-latest}
work_dir=

usage() {
  cat <<'EOF'
Usage: scripts/install.sh [VERSION|latest]

Downloads a postgresem GitHub release, verifies its keyless Sigstore
signature and SHA-256 checksum, and installs it without sudo. VERSION may be
written with or without a leading v.

Environment:
  POSTGRESEM_INSTALL_DIR  Destination directory (default: $HOME/.local/bin)
  POSTGRESEM_REPOSITORY   GitHub owner/repository (default: rioriost/postgresem)
EOF
}

die() {
  printf '%s: %s\n' "$program installer" "$*" >&2
  exit 1
}

cleanup() {
  if [ -n "$work_dir" ] && [ -d "$work_dir" ]; then
    rm -rf "$work_dir"
  fi
}

trap cleanup 0
trap 'exit 1' 1 2 3 15

case ${1:-} in
  -h|--help)
    usage
    exit 0
    ;;
esac

if [ "$#" -gt 1 ]; then
  usage >&2
  exit 2
fi

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v awk >/dev/null 2>&1 || die "awk is required"
command -v cosign >/dev/null 2>&1 || die "cosign is required"
command -v mktemp >/dev/null 2>&1 || die "mktemp is required"

case "$repository" in
  */*) ;;
  *) die "POSTGRESEM_REPOSITORY must use owner/repository form" ;;
esac
case "$repository" in
  *[!A-Za-z0-9._/-]*|*//*|/*|*/|*/*/*)
    die "POSTGRESEM_REPOSITORY contains unsupported characters"
    ;;
esac

case "$(uname -s)" in
  Darwin) asset_os=darwin ;;
  Linux) asset_os=linux ;;
  *) die "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) asset_arch=amd64 ;;
  arm64|aarch64) asset_arch=arm64 ;;
  *) die "unsupported CPU architecture: $(uname -m)" ;;
esac

if [ "$requested_version" = latest ]; then
  latest_url=$(
    curl --proto '=https' --tlsv1.2 \
      --fail --silent --show-error --location \
      --output /dev/null --write-out '%{url_effective}' \
      "https://github.com/${repository}/releases/latest"
  ) || die "could not resolve the latest release"
  latest_url=${latest_url%/}
  tag=${latest_url##*/}
else
  case "$requested_version" in
    v*) tag=$requested_version ;;
    *) tag=v$requested_version ;;
  esac
fi

case "$tag" in
  v[A-Za-z0-9]*)
    case "$tag" in
      *[!A-Za-z0-9._-]*) die "release version contains unsupported characters" ;;
    esac
    ;;
  *) die "release version must start with v and contain a version" ;;
esac

version=${tag#v}
archive="postgresem-${version}-${asset_os}-${asset_arch}.tar.gz"
archive_root=${archive%.tar.gz}
base_url="https://github.com/${repository}/releases/download/${tag}"

case "$install_dir" in
  /*) ;;
  *) install_dir=$PWD/$install_dir ;;
esac
mkdir -p "$install_dir" || die "could not create install directory: $install_dir"
install_dir=$(CDPATH='' cd "$install_dir" && pwd -P) ||
  die "could not resolve install directory"

umask 077
work_dir=$(mktemp -d "${install_dir}/.postgresem-install.XXXXXX") ||
  die "could not create a staging directory in $install_dir"
archive_path=$work_dir/$archive
checksums_path=$work_dir/SHA256SUMS
signature_path=$work_dir/SHA256SUMS.sig
certificate_path=$work_dir/SHA256SUMS.pem
listing_path=$work_dir/archive-contents
verbose_listing_path=$work_dir/archive-types
extract_dir=$work_dir/extracted
mkdir "$extract_dir"

curl --proto '=https' --tlsv1.2 \
  --fail --silent --show-error --location \
  --retry 3 --connect-timeout 15 \
  --output "$checksums_path" "${base_url}/SHA256SUMS" ||
  die "could not download SHA256SUMS"
curl --proto '=https' --tlsv1.2 \
  --fail --silent --show-error --location \
  --retry 3 --connect-timeout 15 \
  --output "$signature_path" "${base_url}/SHA256SUMS.sig" ||
  die "could not download SHA256SUMS.sig"
curl --proto '=https' --tlsv1.2 \
  --fail --silent --show-error --location \
  --retry 3 --connect-timeout 15 \
  --output "$certificate_path" "${base_url}/SHA256SUMS.pem" ||
  die "could not download SHA256SUMS.pem"

certificate_identity="https://github.com/${repository}/.github/workflows/release.yml@refs/tags/${tag}"
cosign verify-blob \
  --certificate "$certificate_path" \
  --signature "$signature_path" \
  --certificate-identity "$certificate_identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$checksums_path" >/dev/null ||
  die "Sigstore verification failed for SHA256SUMS"

curl --proto '=https' --tlsv1.2 \
  --fail --silent --show-error --location \
  --retry 3 --connect-timeout 15 \
  --output "$archive_path" "${base_url}/${archive}" ||
  die "could not download ${archive}"

expected=$(
  awk -v name="$archive" '
    $2 == name {
      count += 1
      checksum = $1
    }
    END {
      if (count != 1) {
        exit 1
      }
      print checksum
    }
  ' "$checksums_path"
) || die "SHA256SUMS does not contain exactly one entry for ${archive}"

if [ "${#expected}" -ne 64 ]; then
  die "release checksum has an invalid length"
fi
case "$expected" in
  *[!A-Fa-f0-9]*) die "release checksum is not hexadecimal" ;;
esac

if command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$archive_path" | awk '{ print $1 }')
elif command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$archive_path" | awk '{ print $1 }')
else
  die "shasum or sha256sum is required for checksum verification"
fi

if [ "$actual" != "$expected" ]; then
  die "checksum verification failed for ${archive}"
fi

tar -tzf "$archive_path" >"$listing_path" ||
  die "could not inspect ${archive}"
LC_ALL=C tar -tvzf "$archive_path" >"$verbose_listing_path" ||
  die "could not inspect archive entry types"
awk '
  substr($0, 1, 1) != "-" && substr($0, 1, 1) != "d" {
    exit 1
  }
' "$verbose_listing_path" ||
  die "archive contains a link or unsupported entry type"
awk -v root="${archive_root}/" '
  {
    path = $0
    sub(/^\.\//, "", path)
    if (index(path, root) != 1) {
      exit 1
    }
    count += 1
    components = split(path, part, "/")
    for (index_ = 1; index_ <= components; index_ += 1) {
      if (part[index_] == "..") {
        exit 1
      }
    }
  }
  END {
    if (count == 0) {
      exit 1
    }
  }
' "$listing_path" || die "archive contains an unsafe path"

tar -xzf "$archive_path" -C "$extract_dir" ||
  die "could not extract ${archive}"
binary=$extract_dir/$archive_root/postgresem
[ -f "$binary" ] || die "archive does not contain the postgresem binary"
[ ! -L "$binary" ] || die "archive binary must not be a symbolic link"

staged_binary=$install_dir/.postgresem.new.$$
mv "$binary" "$staged_binary" ||
  die "could not stage postgresem in $install_dir"
chmod 0755 "$staged_binary" ||
  die "could not make the installed binary executable"
mv -f "$staged_binary" "$install_dir/postgresem" ||
  die "could not install postgresem in $install_dir"

printf 'Installed postgresem %s to %s\n' "$version" "$install_dir/postgresem"
