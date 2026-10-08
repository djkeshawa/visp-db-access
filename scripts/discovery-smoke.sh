#!/usr/bin/env bash
# Sourced by e2e-smoke.sh after its authenticated core checks.
fixture=$VDA_DISCOVERY_FAKE_FIXTURE
checked_in_fixture="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/fixtures/rds-discovery.json"
if [[ ! -f $fixture || ! -w $fixture || $fixture -ef $checked_in_fixture ]]; then
  printf 'Discovery smoke needs a writable temporary fixture copy shared with the server.\n' >&2
  exit 1
fi
discovery_fixture_backup="$work_dir/discovery-fixture.json"
cp -- "$fixture" "$discovery_fixture_backup"
source_body=$(jq -n --arg name "Smoke discovery $run_id" --arg project "$project_id" '{provider:"aws",name:$name,regions:["us-east-1","eu-west-1"],default_project_id:$project,enabled:false,external_id:"smoke-write-only-external-id"}')
call POST /discovery/sources/test 200 "$admin_jar" "$source_body"
check '.ok == true'
call POST /discovery/sources 200 "$admin_jar" "$source_body"
check '.external_id_set == true and .last_test.ok == true and .last_test.tested_at != null and (has("external_id") | not)'
source_id=$(jq -er '.id' "$response")
call GET /discovery/sources 403 "$member_jar"
call POST "/discovery/sources/$source_id/test" 200 "$admin_jar"
check '.ok == true and .identity_arn == "arn:aws:sts::123456789012:assumed-role/visp-db-access-fake/discovery" and ([.regions[].ok] | all)'
call POST "/discovery/sources/$source_id/scan" 200 "$admin_jar"
check '.status == "succeeded" and .found >= 4 and .new >= 4'
call GET "/discovery/resources?source_id=$source_id&limit=100" 200 "$admin_jar"
check '(.items | length) >= 4 and .counts.imported == 0 and .next_cursor == null'
resource_id=$(jq -er '.items[] | select(.identifier == "local-shop-aurora") | .id' "$response")
demo_host=$(jq -er '.items[] | select(.identifier == "local-shop-aurora") | .host' "$response")
if [[ $demo_host != 127.0.0.1 ]]; then
  printf 'Discovery demo fixture must initially use 127.0.0.1.\n' >&2
  exit 1
fi
call POST "/discovery/resources/$resource_id/ignore" 200 "$admin_jar"
check '.status == "ignored"'
call POST "/discovery/resources/$resource_id/unignore" 200 "$admin_jar"
check '.status == "new"'
import_body=$(jq -n --arg project "$project_id" --arg name "Discovered shop $run_id" --arg username "$reader" --arg password "$reader_password" '{project_id:$project,name:$name,environment:"development",database:"shop",username:$username,password:$password,tls_mode:"disable"}')
call POST "/discovery/resources/$resource_id/import" 200 "$admin_jar" "$import_body"
check ".provider == \"aws\" and .engine == \"postgres\" and .host == \"127.0.0.1\" and .port == $target_port and .database == \"shop\" and .tags[\"aws:account\"] == \"123456789012\" and (.tags[\"aws:arn\"] | contains(\"local-shop-aurora\")) and (has(\"password\") | not)"
discovery_cluster_id=$(jq -er '.id' "$response")
call POST "/discovery/resources/$resource_id/import" 409 "$admin_jar" "$import_body"
call POST "/clusters/$discovery_cluster_id/query" 200 "$admin_jar" '{"sql":"SELECT id, name FROM products ORDER BY id LIMIT 1"}'
check '.row_count == 1 and .rows[0][0] == 1'
pass 'discovery source, test, scan, list, ignore/unignore and explicit import/query'

# Both names reach the same demo DB; the changed hostname still requires re-entry.
jq '(.resources[] | select(.identifier == "local-shop-aurora") | .host) = "localhost"' "$discovery_fixture_backup" > "$work_dir/discovery-drift.json"
cat -- "$work_dir/discovery-drift.json" > "$fixture"
call POST "/discovery/sources/$source_id/scan" 200 "$admin_jar"
check '.status == "succeeded" and .new == 0 and .changed >= 1'
call GET "/discovery/resources?source_id=$source_id&status=imported" 200 "$admin_jar"
check '(.items | length) == 1 and .items[0].status == "imported" and ([.items[0].drift[]] | index("endpoint_changed") != null)'
call POST "/discovery/resources/$resource_id/sync" 400 "$admin_jar" '{}'
check '.error.code == "validation" and (.error.message | contains("re-entering the password"))'
call POST "/discovery/resources/$resource_id/sync" 200 "$admin_jar" "$(jq -n --arg password "$reader_password" '{password:$password}')"
check '.host == "localhost" and (has("password") | not)'
call GET "/discovery/resources?source_id=$source_id&status=imported" 200 "$admin_jar"
check '.items[0].drift == []'
call POST "/clusters/$discovery_cluster_id/query" 200 "$admin_jar" '{"sql":"SELECT id FROM products ORDER BY id LIMIT 1"}'
check '.row_count == 1'
call GET "/discovery/sources/$source_id/runs?limit=1" 200 "$admin_jar"
check '(.items | length) == 1 and .next_cursor != null'

# Restore the shared fixture and remove this source; its imported cluster survives.
cat -- "$discovery_fixture_backup" > "$fixture"
discovery_fixture_backup=''
call DELETE "/discovery/sources/$source_id" 204 "$admin_jar"
call GET "/clusters/$discovery_cluster_id" 200 "$admin_jar"
check '.host == "localhost"'
pass 'discovery drift, password re-entry, sync, run pagination and source deletion'
