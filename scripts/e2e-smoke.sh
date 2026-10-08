#!/usr/bin/env bash
# Exercises a running gateway and seeded demo target; leaves uniquely named API fixtures.
set -euo pipefail
umask 077
for tool in curl jq; do command -v "$tool" >/dev/null || { printf 'Missing tool: %s\n' "$tool" >&2; exit 1; }; done
base_url=${VDA_SMOKE_BASE_URL:-http://127.0.0.1:8080}
admin_email=${VDA_SMOKE_ADMIN_EMAIL:-admin@local.test}
admin_password=${VDA_SMOKE_ADMIN_PASSWORD:-local-admin-password}
target_host=${VDA_SMOKE_TARGET_HOST:-127.0.0.1}
target_port=${VDA_SMOKE_TARGET_PORT:-55432}
target_db=${VDA_SMOKE_TARGET_DATABASE:-shop}
reader=${VDA_SMOKE_READER:-shop_reader}
reader_password=${VDA_SMOKE_READER_PASSWORD:-shop_reader_pw}
writer=${VDA_SMOKE_WRITER:-shop_writer}
writer_password=${VDA_SMOKE_WRITER_PASSWORD:-shop_writer_pw}
run_id="$(date +%s)-$$"
work_dir=$(mktemp -d)
cleanup() {
  if [[ -n ${mysql_request_pid:-} ]] && kill -0 "$mysql_request_pid" 2>/dev/null; then
    curl --silent --show-error --max-time 5 --request POST --header 'X-Requested-With: vda' \
      --cookie "$admin_jar" "$base_url/api/v1/queries/$mysql_cancel_id/cancel" >/dev/null 2>&1 || true
    kill "$mysql_request_pid" 2>/dev/null || true
    wait "$mysql_request_pid" 2>/dev/null || true
  fi
  if [[ -n ${discovery_fixture_backup:-} ]]; then
    cat -- "$discovery_fixture_backup" > "$VDA_DISCOVERY_FAKE_FIXTURE"
  fi
  rm -rf -- "$work_dir"
}
trap cleanup EXIT
admin_jar="$work_dir/admin.cookies"
reviewer_jar="$work_dir/reviewer.cookies"
member_jar="$work_dir/member.cookies"
response="$work_dir/response.json"

call() {
  local method=$1 path=$2 expected=$3 jar=$4 body=${5-}
  local -a args=(--silent --show-error --max-time 90 --request "$method"
    --header 'X-Requested-With: vda' --cookie "$jar" --cookie-jar "$jar"
    --output "$response" --write-out '%{http_code}')
  if [[ -n $body ]]; then args+=(--header 'Content-Type: application/json' --data-binary @-); fi
  local status
  status=$(curl "${args[@]}" "$base_url/api/v1$path" <<< "$body")
  if [[ $status != "$expected" ]]; then
    printf 'FAIL %s %s: expected %s, received %s\n' "$method" "$path" "$expected" "$status" >&2
    cat "$response" >&2
    exit 1
  fi
}
check() {
  if ! jq --exit-status "$1" "$response" >/dev/null; then
    printf 'FAIL response assertion: %s\n' "$1" >&2; cat "$response" >&2; exit 1
  fi
}
pass() { printf 'PASS %s\n' "$1"; }
login_body=$(jq -n --arg email "$admin_email" --arg password "$admin_password" '{email:$email,password:$password}')
call POST /auth/login 200 "$admin_jar" "$login_body"
check '.user.org_role == "admin" and (.user | has("password_hash") | not)'
admin_id=$(jq -r '.user.id' "$response")
pass login

call POST /projects 200 "$admin_jar" "$(jq -n --arg name "Smoke $run_id" '{name:$name,description:"Real API smoke test"}')"
project_id=$(jq -er '.id' "$response")
cluster_body=$(jq -n --arg project "$project_id" --arg host "$target_host" --argjson port "$target_port" --arg database "$target_db" --arg username "$reader" --arg password "$reader_password" '{project_id:$project,name:"Smoke reader",engine:"postgres",provider:"onprem",region:"local",environment:"development",host:$host,port:$port,database:$database,username:$username,password:$password,tls_mode:"disable"}')
call POST /clusters 200 "$admin_jar" "$cluster_body"
cluster_id=$(jq -er '.id' "$response")
check 'has("password") == false and has("password_enc") == false'
pass 'project and least-privileged reader cluster'

call POST /clusters/test-connection 200 "$admin_jar" "$(jq --arg id "$cluster_id" 'del(.project_id,.name,.provider,.region,.environment,.password) + {cluster_id:$id}' <<< "$cluster_body")"
check '.status == "healthy"'
call POST "/clusters/$cluster_id/health/check" 200 "$admin_jar"
check '.status == "healthy"'
call GET "/clusters/$cluster_id/health" 200 "$admin_jar"
check '.current.status == "healthy" and (.history | length) > 0'
call GET "/clusters/$cluster_id/schema" 200 "$admin_jar"
check '[.schemas[].tables[].name] | index("products") != null'
pass 'connection, health and schema'

call GET "/clusters/$cluster_id/policy" 200 "$admin_jar"
policy=$(jq '.max_rows=2 | .max_cost=null | .allow_writes=true | .require_approval_for_writes=true | .masked_columns=["email"]' "$response")
call PUT "/clusters/$cluster_id/policy" 200 "$admin_jar" "$policy"
read_body='{"sql":"SELECT id,name,email FROM customers ORDER BY id"}'
call POST "/clusters/$cluster_id/analyze" 200 "$admin_jar" "$read_body"
check '.verdict == "allow" and (.rewritten_sql | contains("LIMIT 3"))'
call POST "/clusters/$cluster_id/query" 200 "$admin_jar" "$read_body"
check '.row_count == 2 and .truncated == true and .rows[0][2] == "••••••" and .routed_to == "primary"'
read_query_id=$(jq -er '.query_id' "$response")
pass 'SELECT, masking and max_rows+1 truncation'
call POST "/clusters/$cluster_id/query" 422 "$admin_jar" '{"sql":"DELETE FROM products"}'
check '.error.code == "query_denied"'
call POST "/clusters/$cluster_id/analyze" 200 "$admin_jar" '{"sql":"SELECT row_to_json(customers) FROM customers"}'
check '.verdict == "deny" and ([.issues[].code] | index("masked_data_serialization") != null)'
call PATCH "/clusters/$cluster_id" 400 "$admin_jar" '{"host":"localhost"}'
check '.error.code == "validation" and (.error.message | contains("re-entering the password"))'
call POST /projects 400 "$admin_jar" '{"name":"bad\u0000","description":"control regression"}'
check '.error.code == "validation"'
pass 'blocked DELETE, serialization guard and input/endpoint validation'

# Separate scoped approver and writer users exercise lookup and RBAC as well as four-eyes.
reviewer_email="smoke-reviewer-$run_id@local.test"
member_email="smoke-member-$run_id@local.test"
user_password="smoke-password-$run_id"
for account in reviewer member; do
  if [[ $account == reviewer ]]; then email=$reviewer_email; else email=$member_email; fi
  call POST /users 200 "$admin_jar" "$(jq -n --arg email "$email" --arg name "Smoke $account $run_id" --arg password "$user_password" '{email:$email,name:$name,password:$password,org_role:"member"}')"
  if [[ $account == reviewer ]]; then reviewer_id=$(jq -er '.id' "$response"); else member_id=$(jq -er '.id' "$response"); fi
 done
call POST /grants 200 "$admin_jar" "$(jq -n --arg user "$reviewer_id" --arg project "$project_id" '{user_id:$user,scope:"project",scope_id:$project,level:"admin"}')"
call POST /grants 200 "$admin_jar" "$(jq -n --arg user "$member_id" --arg project "$project_id" '{user_id:$user,scope:"project",scope_id:$project,level:"write"}')"
call POST /auth/login 200 "$reviewer_jar" "$(jq -n --arg email "$reviewer_email" --arg password "$user_password" '{email:$email,password:$password}')"
call POST /auth/login 200 "$member_jar" "$(jq -n --arg email "$member_email" --arg password "$user_password" '{email:$email,password:$password}')"
call GET "/users/lookup?q=smoke-member-$run_id" 200 "$reviewer_jar"
check "(.items | length) == 1 and .items[0].id == \"$member_id\" and (.items[0] | keys | length) == 3"
call GET /users 403 "$reviewer_jar"
call GET "/grants?scope=project&scope_id=$project_id&limit=1" 200 "$reviewer_jar"
check '(.items | length) == 1 and .next_cursor != null'
cursor=$(jq -r '.next_cursor | @uri' "$response")
first_grant_id=$(jq -r '.items[0].id' "$response")
call GET "/grants?scope=project&scope_id=$project_id&limit=1&cursor=$cursor" 200 "$reviewer_jar"
check "(.items | length) == 1 and .items[0].id != \"$first_grant_id\" and .next_cursor == null"
call GET /grants 403 "$member_jar"
pass 'scoped grant pagination and minimal user lookup'

# An approved write against the SELECT-only role must fail and remain consumed.
call POST /approvals 200 "$member_jar" "$(jq -n --arg cluster "$cluster_id" '{cluster_id:$cluster,sql:"UPDATE products SET price=price WHERE id=1",reason:"Smoke: reader cannot write"}')"
failed_approval_id=$(jq -er '.id' "$response")
call POST "/approvals/$failed_approval_id/approve" 200 "$reviewer_jar" '{}'
call POST "/approvals/$failed_approval_id/execute" 502 "$member_jar"
check '.error.code == "upstream"'
call GET "/approvals/$failed_approval_id" 200 "$member_jar"
check '.status == "failed" and (.error | length) > 0 and .result == null'
call POST "/approvals/$failed_approval_id/execute" 409 "$member_jar"
pass 'reader write rejection and failed approval consumed once'

# Rotate explicitly to the write role; a SELECT-only role must never apply a write.
call PATCH "/clusters/$cluster_id" 200 "$admin_jar" "$(jq -n --arg username "$writer" --arg password "$writer_password" '{username:$username,password:$password}')"
write_sql='UPDATE products SET price=price WHERE id=1'
write_body=$(jq -n --arg sql "$write_sql" '{sql:$sql}')
call POST "/clusters/$cluster_id/query" 409 "$member_jar" "$write_body"
check '.error.code == "approval_required"'
# SQL over the summary bound proves that the detail endpoint retains the full query.
long_sql="$write_sql /* $(printf '%2100s' '' | tr ' ' x) */"
call POST /approvals 200 "$member_jar" "$(jq -n --arg cluster "$cluster_id" --arg sql "$long_sql" '{cluster_id:$cluster,sql:$sql,reason:"Smoke: verify four-eyes write"}')"
approval_id=$(jq -er '.id' "$response")
call GET "/approvals?cluster_id=$cluster_id&limit=1" 200 "$reviewer_jar"
check '.items[0].sql_truncated == true and .items[0].result == null and (.items[0].sql | length) == 2000'
check '.next_cursor != null'
approval_cursor=$(jq -r '.next_cursor | @uri' "$response")
call GET "/approvals?cluster_id=$cluster_id&limit=1&cursor=$approval_cursor" 200 "$reviewer_jar"
check '.items[0].status == "failed" and .next_cursor == null'

call GET "/approvals/$approval_id" 200 "$reviewer_jar"
check '.sql_truncated == false and (.sql | length) > 2000'
call POST "/approvals/$approval_id/approve" 403 "$member_jar" '{}'
call POST "/approvals/$approval_id/approve" 200 "$reviewer_jar" '{"note":"Verified one-row predicate"}'
check '.status == "approved"'
call POST "/approvals/$approval_id/execute" 200 "$member_jar"
check '.status == "executed" and .error == null and .result.affected_rows == 1'
write_query_id=$(jq -er '.result.query_id' "$response")
call POST "/approvals/$approval_id/execute" 409 "$member_jar"
pass 'write approval, second-user review, execution and replay rejection'

call GET "/history?cluster_id=$cluster_id&limit=200" 200 "$admin_jar"
check '([.items[] | select(.status == "ok" and (.sql | startswith("SELECT id,name,email")))] | length) == 1 and ([.items[] | select(.status == "ok" and (.sql | startswith("UPDATE products")))] | length) == 1 and ([.items[] | select(.status == "blocked")] | length) >= 1'
# Audit uses actor IDs to avoid unrelated fixture activity; intent and completion must persist.
call GET "/audit?actor_id=$admin_id&limit=200" 200 "$admin_jar"
check "([.items[] | select(.action == \"query.execute\" and .details.query_id == \"$read_query_id\" and .details.status == \"running\")] | length) == 1 and ([.items[] | select(.action == \"query.execute\" and .target_id == \"$cluster_id\" and .details.status == \"blocked\")] | length) >= 1"
call GET "/audit?actor_id=$member_id&limit=200" 200 "$admin_jar"
check "([.items[].action] | index(\"approval.create\") != null) and ([.items[].action] | index(\"approval.executed\") != null) and ([.items[] | select(.action == \"query.execute\" and .details.query_id == \"$write_query_id\") ] | length) == 1"
call GET "/audit?actor_id=$reviewer_id&limit=200" 200 "$admin_jar"
check '[.items[].action] | index("approval.approve") != null'
pass 'history and durable audit'
if [[ -n ${VDA_SMOKE_MYSQL_HOST:-} ]]; then
  source "$(dirname -- "${BASH_SOURCE[0]}")/mysql-smoke.sh"
else
  printf 'SKIP MySQL: VDA_SMOKE_MYSQL_HOST is unset.\n'
fi
if [[ -n ${VDA_DISCOVERY_FAKE_FIXTURE:-} ]]; then
  # The running fake provider must use this same writable temporary fixture.
  source "$(dirname -- "${BASH_SOURCE[0]}")/discovery-smoke.sh"
else
  printf 'SKIP discovery: VDA_DISCOVERY_FAKE_FIXTURE is unset.\n'
fi
for jar in "$admin_jar" "$reviewer_jar" "$member_jar"; do call POST /auth/logout 204 "$jar"; done
printf 'SMOKE OK: login → project → cluster → connection → health → schema → analyze → SELECT → blocked DELETE → approval → review → execute → history/audit\n'
printf 'Fixtures retained: project=%s cluster=%s approval=%s\n' "$project_id" "$cluster_id" "$approval_id"
