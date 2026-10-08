#!/usr/bin/env bash
# Optional section sourced by e2e-smoke.sh; uses its auth, project and helper functions.
# Enable with VDA_SMOKE_MYSQL_HOST (defaults: port 33306, database shop,
# shop_reader/shop_reader_pw, shop_writer/shop_writer_pw). No mysql CLI required.
mysql_body=$(jq -n --arg project "$project_id" --arg host "$VDA_SMOKE_MYSQL_HOST" \
  --argjson port "${VDA_SMOKE_MYSQL_PORT:-33306}" --arg database "${VDA_SMOKE_MYSQL_DATABASE:-shop}" \
  --arg username "${VDA_SMOKE_MYSQL_READER:-shop_reader}" --arg password "${VDA_SMOKE_MYSQL_READER_PASSWORD:-shop_reader_pw}" \
  '{project_id:$project,name:"Smoke MySQL",engine:"mysql",provider:"onprem",region:"local",environment:"development",host:$host,port:$port,database:$database,username:$username,password:$password,tls_mode:"disable"}')
call POST /clusters 200 "$admin_jar" "$mysql_body"
mysql_cluster=$(jq -er '.id' "$response")
call POST /clusters/test-connection 200 "$admin_jar" "$(jq --arg id "$mysql_cluster" 'del(.project_id,.name,.provider,.region,.environment,.password) + {cluster_id:$id}' <<< "$mysql_body")"
check '.status == "healthy" and .server_version != null'
call POST "/clusters/$mysql_cluster/health/check" 200 "$admin_jar"
check '.status == "healthy" and .is_replica == false'
call GET "/clusters/$mysql_cluster/health" 200 "$admin_jar"
check '.current.status == "healthy" and (.history | length) > 0'
call GET "/clusters/$mysql_cluster/schema?refresh=true" 200 "$admin_jar"
check '[.schemas[].tables[].name] | index("products") != null'
call GET "/clusters/$mysql_cluster/policy" 200 "$admin_jar"
mysql_policy=$(jq '.max_rows=2 | .max_cost=null | .allow_writes=true | .require_approval_for_writes=true | .max_affected_rows=1 | .masked_columns=["email"]' "$response")
call PUT "/clusters/$mysql_cluster/policy" 200 "$admin_jar" "$mysql_policy"
call POST "/clusters/$mysql_cluster/query" 200 "$admin_jar" '{"sql":"SELECT id,name,email FROM customers ORDER BY id"}'
check '.row_count == 2 and .truncated == true and .rows[0][2] == "••••••"'
call POST "/clusters/$mysql_cluster/query" 422 "$admin_jar" '{"sql":"DELETE FROM products"}'
check '.error.code == "query_denied"'
call POST "/clusters/$mysql_cluster/query" 422 "$admin_jar" '{"sql":"SELECT 1 /*!50000 INTO @bypass */"}'
check '.error.code == "query_denied" and ([.error.details.issues[].code] | index("executable_comment") != null)'
pass 'MySQL connection, health, schema, masking, truncation, blocked DELETE and executable comment'

# Aggregate forces real work despite the rewritten LIMIT. No SLEEP/BENCHMARK bypass.
mysql_cancel_id=$(cat /proc/sys/kernel/random/uuid)
mysql_slow='SELECT SUM(a.id+b.id+c.id+d.id) FROM products a CROSS JOIN products b CROSS JOIN products c CROSS JOIN products d'
jq -n --arg sql "$mysql_slow" --arg id "$mysql_cancel_id" '{sql:$sql,query_id:$id}' > "$work_dir/mysql-slow.json"
curl --silent --show-error --max-time 90 --request POST --header 'X-Requested-With: vda' \
  --header 'Content-Type: application/json' --cookie "$admin_jar" --data-binary @"$work_dir/mysql-slow.json" \
  --output "$work_dir/mysql-cancel.json" --write-out '%{http_code}' \
  "$base_url/api/v1/clusters/$mysql_cluster/query" > "$work_dir/mysql-cancel.status" &
mysql_request_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
  call GET "/history?cluster_id=$mysql_cluster&status=running" 200 "$admin_jar"
  if jq -e '.items | length > 0' "$response" >/dev/null; then break; fi
  sleep 0.1
done
# Allow target execution to start after the durable intent record.
sleep 0.3
call POST "/queries/$mysql_cancel_id/cancel" 204 "$admin_jar"
wait "$mysql_request_pid"
mysql_request_pid=
[[ $(cat "$work_dir/mysql-cancel.status") == 502 ]] || { cat "$work_dir/mysql-cancel.json" >&2; exit 1; }
cp "$work_dir/mysql-cancel.json" "$response"
check '.error.code == "upstream" and (.error.message | contains("cancelled"))'
call GET "/history?cluster_id=$mysql_cluster&status=cancelled" 200 "$admin_jar"
check '.items | length == 1'
call POST "/clusters/$mysql_cluster/query" 200 "$admin_jar" '{"sql":"SELECT 42"}'
check '.rows[0][0] == 42'
pass 'MySQL heavy cross join cancelled through endpoint and next query succeeds'

# An approval cannot manufacture privileges for the SELECT-only target role.
call POST /approvals 200 "$member_jar" "$(jq -n --arg cluster "$mysql_cluster" '{cluster_id:$cluster,sql:"UPDATE products SET price=price WHERE id=1",reason:"MySQL reader rejects writes"}')"
mysql_approval=$(jq -er '.id' "$response")
call POST "/approvals/$mysql_approval/approve" 200 "$reviewer_jar" '{}'
call POST "/approvals/$mysql_approval/execute" 502 "$member_jar"
call GET "/approvals/$mysql_approval" 200 "$member_jar"
check '.status == "failed"'
call PATCH "/clusters/$mysql_cluster" 200 "$admin_jar" "$(jq -n --arg user "${VDA_SMOKE_MYSQL_WRITER:-shop_writer}" --arg password "${VDA_SMOKE_MYSQL_WRITER_PASSWORD:-shop_writer_pw}" '{username:$user,password:$password}')"
call POST "/clusters/$mysql_cluster/query" 409 "$member_jar" '{"sql":"UPDATE products SET price=price WHERE id=1"}'
for cap_test in success rollback; do
  if [[ $cap_test == success ]]; then mysql_sql='UPDATE products SET price=price WHERE id=1'; else mysql_sql='UPDATE products SET price=price+100 WHERE id IN (1,2)'; fi
  call POST /approvals 200 "$member_jar" "$(jq -n --arg cluster "$mysql_cluster" --arg sql "$mysql_sql" '{cluster_id:$cluster,sql:$sql,reason:"MySQL affected-row budget"}')"
  mysql_approval=$(jq -er '.id' "$response")
  call POST "/approvals/$mysql_approval/approve" 200 "$reviewer_jar" '{}'
  if [[ $cap_test == success ]]; then
    call POST "/approvals/$mysql_approval/execute" 200 "$member_jar"
    check '.status == "executed" and .result.affected_rows == 1'
  else
    call POST "/clusters/$mysql_cluster/query" 200 "$admin_jar" '{"sql":"SELECT id,price FROM products WHERE id IN (1,2) ORDER BY id"}'
    mysql_before=$(jq -c '.rows' "$response")
    call POST "/approvals/$mysql_approval/execute" 502 "$member_jar"
    check '.error.message | contains("transaction rolled back")'
    call GET "/approvals/$mysql_approval" 200 "$member_jar"
    check '.status == "failed"'
    call POST "/clusters/$mysql_cluster/query" 200 "$admin_jar" '{"sql":"SELECT id,price FROM products WHERE id IN (1,2) ORDER BY id"}'
    [[ $(jq -c '.rows' "$response") == "$mysql_before" ]] || { printf 'MySQL cap did not roll back\n' >&2; exit 1; }
  fi
  call POST "/approvals/$mysql_approval/execute" 409 "$member_jar"
done
pass 'MySQL least-privilege reader, gated writer approval, affected-row rollback and replay rejection'
