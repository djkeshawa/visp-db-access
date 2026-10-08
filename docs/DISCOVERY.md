# AWS discovery

Organization admins can add AWS discovery sources, test access, and scan
selected regions for RDS and Aurora databases. Discovered databases appear under
**Clusters → Discovered**, where an admin can review and import them.

## How access works

Discovery uses the AWS SDK's default credential chain: IRSA on EKS, ECS task
roles, EC2 instance profiles, or credentials supplied to the process. The
gateway stores no AWS access keys. A source can optionally assume a role in
another account for one hour per scan. Its external ID is encrypted with the
gateway's master key, and the API only reports whether one is set.

### Same account

Attach [the discovery IAM policy](../deploy/aws/iam-policy.json) to the
gateway's role. It grants only `rds:DescribeDBInstances` and
`rds:DescribeDBClusters` (inventory APIs require `Resource: "*"`). You can
restrict regions with an `aws:RequestedRegion` condition. See the
[RDS authorization reference](https://docs.aws.amazon.com/service-authorization/latest/reference/list_rds.html).

### Cross-account

1. In the target account, create a role from
   [the trust policy example](../deploy/aws/cross-account-role.json). Replace
   the gateway role ARN and the external ID. Name the gateway role, not the
   whole account, and use an external ID unique to this gateway and source.
2. Attach the discovery IAM policy to that target role.
3. Allow `sts:AssumeRole` on that exact role ARN in the gateway identity's
   policy.
4. In **Settings → Cloud discovery**, add a source with the role ARN, external
   ID and regions to scan, and use **Test** to confirm the identity.

### EKS IRSA

Configure your cluster's OIDC provider and an IAM role whose trust policy allows
the Helm release's service account, then set:

```yaml
serviceAccount:
  create: true
  annotations:
    eks.amazonaws.com/role-arn: arn:aws:iam::123456789012:role/visp-db-access
```

To reuse an existing annotated account, set `serviceAccount.create: false` and
`serviceAccount.name`. The chart keeps Kubernetes API token automounting
disabled; EKS injects the separate AWS web-identity token. See
[AWS service-account configuration](https://docs.aws.amazon.com/eks/latest/userguide/pod-configuration.html).

## What scans find

- PostgreSQL, Aurora PostgreSQL, MySQL, Aurora MySQL (including legacy
  `aurora`) and MariaDB.
- Aurora clusters use their writer endpoint, plus their reader endpoint when
  they have reader instances. Member instances are not listed separately.
- RDS read replicas attach to their source database rather than appearing as
  separate rows.
- Instances without an endpoint yet, and unsupported engines (Oracle, SQL
  Server, Db2, DocumentDB, Neptune), are skipped.

Discovery never provisions infrastructure or obtains database passwords.
Enabled sources rescan automatically at their configured interval (5 to 1,440
minutes) on one gateway node.

## Import and drift

Importing is always an explicit admin action: choose a project and environment,
enter database credentials, and use `verify_full` TLS for cloud targets.
Imported clusters get the same validation, credential encryption, policy
defaults and audit records as manually added ones.

Later scans flag endpoint or replica changes and deleted databases. A region
that fails to scan never marks its databases as gone. Syncing a changed endpoint
requires re-entering the database password; engine changes and deletions stay
visible for manual review.

## Developing without AWS

The server has a `fake-discovery` feature that reads a fixture file instead of
calling AWS. It is excluded from normal builds and logs a warning at startup;
never enable it in production.

```sh
cp scripts/fixtures/rds-discovery.json /tmp/vda-rds-discovery.json
export VDA_DISCOVERY_FAKE_FIXTURE=/tmp/vda-rds-discovery.json
scripts/dev-local.sh
# In another shell, with the same variable exported:
scripts/e2e-smoke.sh
```

The smoke script temporarily edits the fixture to exercise drift and sync, and
restores it on exit.
