#!/bin/sh
set -eu

quadlet_directory=$(CDPATH= cd -- "${1:-deploy/quadlet}" && pwd)
output_file=$(mktemp)
error_file=$(mktemp)
trap 'rm -f "${output_file}" "${error_file}"' EXIT

generator=
for candidate in \
  /usr/lib/systemd/system-generators/podman-system-generator \
  /usr/lib/systemd/user-generators/podman-user-generator \
  /usr/libexec/podman/quadlet
do
  if [ -x "${candidate}" ]; then
    generator=${candidate}
    break
  fi
done

if [ -z "${generator}" ]; then
  echo "Podman Quadlet generator was not installed" >&2
  exit 1
fi

if ! QUADLET_UNIT_DIRS="${quadlet_directory}" \
  "${generator}" --user --dryrun >"${output_file}" 2>"${error_file}"
then
  cat "${error_file}" >&2
  exit 1
fi

if grep -Ei '(^|[[:space:]])(error|warning):' "${error_file}" >/dev/null; then
  cat "${error_file}" >&2
  exit 1
fi

for service in \
  postgresem-db.service \
  postgresem-migrate.service \
  postgresem-seed.service \
  postgresem-gateway.service
do
  if ! grep -F "${service}" "${output_file}" >/dev/null; then
    echo "Quadlet did not generate ${service}" >&2
    exit 1
  fi
done

grep -F "postgresem-gateway.env" \
  "${quadlet_directory}/postgresem-gateway.container" >/dev/null
if grep -F "postgresem-database.env" \
  "${quadlet_directory}/postgresem-gateway.container" >/dev/null
then
  echo "gateway Quadlet received database-administrator credentials" >&2
  exit 1
fi
grep -F "exec /bin/sh /migrations/run.sh" \
  "${quadlet_directory}/postgresem-migrate.container" >/dev/null
grep -F "exec /bin/sh /semantic/run.sh" \
  "${quadlet_directory}/postgresem-seed.container" >/dev/null

echo "Quadlet generation checks passed"
