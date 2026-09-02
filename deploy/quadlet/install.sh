#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
quadlet_directory="${HOME}/.config/containers/systemd"
configuration_directory="${HOME}/.config/postgresem"
data_directory="${HOME}/.local/share/postgresem"

install -d -m 700 \
  "${quadlet_directory}" \
  "${configuration_directory}" \
  "${data_directory}/migrations" \
  "${data_directory}/fixtures/postgres" \
  "${data_directory}/fixtures/semantic"

for unit in \
  postgresem.network \
  postgresem-db.volume \
  postgresem-db.container \
  postgresem-migrate.container \
  postgresem-seed.container \
  postgresem-gateway.container
do
  install -m 600 "${repository_root}/deploy/quadlet/${unit}" \
    "${quadlet_directory}/${unit}"
done

cp -R "${repository_root}/migrations/." "${data_directory}/migrations/"
cp -R "${repository_root}/fixtures/postgres/." \
  "${data_directory}/fixtures/postgres/"
cp -R "${repository_root}/fixtures/semantic/." \
  "${data_directory}/fixtures/semantic/"

for environment in postgresem-database postgresem-gateway
do
  if [ ! -f "${configuration_directory}/${environment}.env" ]; then
    install -m 600 \
      "${repository_root}/deploy/quadlet/${environment}.env.example" \
      "${configuration_directory}/${environment}.env"
    printf '%s\n' \
      "edit ${configuration_directory}/${environment}.env before starting the units"
  fi
done

printf '%s\n' \
  "installed rootless Quadlet files in ${quadlet_directory}" \
  "run: systemctl --user daemon-reload" \
  "run: systemctl --user start postgresem-gateway.service"
